//! Deterministic, renderer-agnostic level editing and preview.
//!
//! [`LevelEditor`] owns an authoring [`LevelDef`] and its last valid compiled
//! [`Level`] at the same time. Commands are transactional: a candidate
//! definition is compiled before it replaces the active preview, so an
//! in-engine editor can reject malformed geometry without ever handing a
//! partially edited world to gameplay.

use glam::Vec2;

use crate::level::{
    AmbienceDef, BossDef, EnemyDef, HazardDef, MoverDef, PickupDef, PowerUpDef, RectDef, SlopeDef,
};
use crate::{Aabb, Level, LevelDef, LevelError};

/// Maximum number of definition snapshots retained for undo.
pub const MAX_UNDO_STEPS: usize = 128;

/// A stable handle for the authoring item under the editor cursor.
///
/// Collection indices refer to the source definition and are intentionally
/// deterministic. A successful delete clears the selection instead of
/// guessing which neighboring item the author meant to edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorSelection {
    None,
    Spawn,
    Bounds,
    Solid(usize),
    OneWay(usize),
    Mover(usize),
    Slope(usize),
    Enemy(usize),
    Hazard(usize),
    Pickup(usize),
    Water(usize),
    Ambience(usize),
    PowerUp(usize),
    Boss,
    Checkpoint(usize),
}

/// Data that can be inserted into a level from an editor palette.
#[derive(Debug, Clone, PartialEq)]
pub enum LevelElement {
    Solid(RectDef),
    OneWay(RectDef),
    Mover(MoverDef),
    Slope(SlopeDef),
    Enemy(EnemyDef),
    Hazard(HazardDef),
    Pickup(PickupDef),
    Water(RectDef),
    Ambience(AmbienceDef),
    PowerUp(PowerUpDef),
    Boss(BossDef),
    Checkpoint(PickupDef),
}

/// One deterministic edit command supplied by an editor shell.
#[derive(Debug, Clone, PartialEq)]
pub enum EditorCommand {
    Select(EditorSelection),
    /// Move the selected item in world units.
    Translate(Vec2),
    /// Change the selected item's size. Rectangles use `(width, height)`;
    /// circular pickups and square actors use the x component.
    Resize(Vec2),
    Add(LevelElement),
    Delete,
}

/// Why an edit could not be applied.
#[derive(Debug, Clone, PartialEq)]
pub enum LevelEditorError {
    NoSelection,
    InvalidSelection(EditorSelection),
    ProtectedSelection(EditorSelection),
    CannotResize(EditorSelection),
    Validation(LevelError),
    Parse(String),
}

impl std::fmt::Display for LevelEditorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSelection => write!(formatter, "no level element is selected"),
            Self::InvalidSelection(selection) => {
                write!(formatter, "invalid level editor selection: {selection:?}")
            }
            Self::ProtectedSelection(selection) => {
                write!(
                    formatter,
                    "level editor selection cannot be deleted: {selection:?}"
                )
            }
            Self::CannotResize(selection) => {
                write!(
                    formatter,
                    "level editor selection cannot be resized: {selection:?}"
                )
            }
            Self::Validation(error) => write!(formatter, "edited level is invalid: {error}"),
            Self::Parse(error) => write!(formatter, "level JSON error: {error}"),
        }
    }
}

impl std::error::Error for LevelEditorError {}

impl From<LevelError> for LevelEditorError {
    fn from(error: LevelError) -> Self {
        Self::Validation(error)
    }
}

impl From<serde_json::Error> for LevelEditorError {
    fn from(error: serde_json::Error) -> Self {
        Self::Parse(error.to_string())
    }
}

/// Result of a successfully applied command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditResult {
    Selected(EditorSelection),
    PreviewUpdated,
}

/// Authoring definition plus its last valid compiled preview.
#[derive(Debug, Clone)]
pub struct LevelEditor {
    definition: LevelDef,
    preview: Level,
    selection: EditorSelection,
    undo: Vec<LevelDef>,
    redo: Vec<LevelDef>,
    saved_definition: LevelDef,
    last_error: Option<LevelEditorError>,
}

impl LevelEditor {
    /// Starts an editor only when the initial definition is valid.
    pub fn new(definition: LevelDef) -> Result<Self, LevelError> {
        let preview = Level::from_def(definition.clone())?;
        Ok(Self {
            saved_definition: definition.clone(),
            definition,
            preview,
            selection: EditorSelection::None,
            undo: Vec::new(),
            redo: Vec::new(),
            last_error: None,
        })
    }

    /// Parses and validates a JSON-authored level for editing.
    pub fn from_json(json: &str) -> Result<Self, LevelEditorError> {
        let definition = LevelDef::from_json(json)?;
        Self::new(definition).map_err(LevelEditorError::Validation)
    }

    /// The editable source definition.
    pub fn definition(&self) -> &LevelDef {
        &self.definition
    }

    /// The last valid compiled preview consumed by simulation/render code.
    pub fn preview(&self) -> &Level {
        &self.preview
    }

    pub fn selection(&self) -> EditorSelection {
        self.selection
    }

    /// Returns the selected item's world-space bounds at an absolute preview
    /// time. Dynamic movers and actors use their authored motion functions;
    /// static items are independent of `time`.
    pub fn selection_bounds_at(&self, time: f32) -> Option<Aabb> {
        match self.selection {
            EditorSelection::None => None,
            EditorSelection::Spawn => Some(self.definition.spawn.aabb()),
            EditorSelection::Bounds => Some(self.definition.bounds.aabb()),
            EditorSelection::Solid(index) => self.definition.solids.get(index).map(RectDef::aabb),
            EditorSelection::OneWay(index) => {
                self.definition.one_ways.get(index).map(RectDef::aabb)
            }
            EditorSelection::Mover(index) => self
                .definition
                .movers
                .get(index)
                .map(|mover| mover.bounds_at(sanitize_preview_time(time))),
            EditorSelection::Slope(index) => self
                .definition
                .slopes
                .get(index)
                .map(|slope| slope.rect.aabb()),
            EditorSelection::Enemy(index) => self
                .definition
                .enemies
                .get(index)
                .map(|enemy| enemy.bounds_at(sanitize_preview_time(time))),
            EditorSelection::Hazard(index) => self
                .definition
                .hazards
                .get(index)
                .map(|hazard| hazard.rect.aabb()),
            EditorSelection::Pickup(index) => self.definition.pickups.get(index).map(|pickup| {
                Aabb::from_center_size(
                    Vec2::new(pickup.x, pickup.y),
                    Vec2::splat(pickup.radius * 2.0),
                )
            }),
            EditorSelection::Water(index) => self.definition.water.get(index).map(RectDef::aabb),
            EditorSelection::Ambience(index) => self
                .definition
                .ambience
                .get(index)
                .map(|ambience| ambience.rect.aabb()),
            EditorSelection::PowerUp(index) => self.definition.powerups.get(index).map(|powerup| {
                Aabb::from_center_size(Vec2::new(powerup.x, powerup.y), Vec2::splat(1.0))
            }),
            EditorSelection::Boss => self
                .definition
                .boss
                .map(|boss| boss.bounds_at(sanitize_preview_time(time), 1.0)),
            EditorSelection::Checkpoint(index) => {
                self.definition.checkpoints.get(index).map(|checkpoint| {
                    Aabb::from_center_size(Vec2::new(checkpoint.x, checkpoint.y), Vec2::splat(1.0))
                })
            }
        }
    }

    /// Returns the last rejected command, if any. A successful command clears
    /// this value so HUDs can show a short-lived validation message.
    pub fn last_error(&self) -> Option<&LevelEditorError> {
        self.last_error.as_ref()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Whether the current definition differs from the last saved snapshot.
    pub fn is_dirty(&self) -> bool {
        self.definition != self.saved_definition
    }

    /// Marks the current valid definition as persisted without discarding
    /// undo/redo history.
    pub fn mark_saved(&mut self) {
        self.saved_definition = self.definition.clone();
        self.last_error = None;
    }

    /// Serializes the current valid definition for a file or browser download.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.definition)
    }

    /// Applies one command transactionally.
    pub fn apply(&mut self, command: EditorCommand) -> Result<EditResult, LevelEditorError> {
        if let EditorCommand::Select(selection) = command {
            let result = self.select(selection);
            if result.is_err() {
                self.last_error = result.as_ref().err().cloned();
            } else {
                self.last_error = None;
            }
            return result.map(EditResult::Selected);
        }

        let previous = self.definition.clone();
        let mut candidate = previous.clone();
        let next_selection = match apply_mutation(&mut candidate, self.selection, command) {
            Ok(selection) => selection,
            Err(error) => {
                self.last_error = Some(error.clone());
                return Err(error);
            }
        };
        let preview = match Level::from_def(candidate.clone()) {
            Ok(preview) => preview,
            Err(error) => {
                let error = LevelEditorError::Validation(error);
                self.last_error = Some(error.clone());
                return Err(error);
            }
        };

        self.push_undo(previous);
        self.definition = candidate;
        self.preview = preview;
        self.selection = next_selection;
        self.redo.clear();
        self.last_error = None;
        Ok(EditResult::PreviewUpdated)
    }

    /// Restores the previous valid definition, returning whether anything was
    /// undone. History snapshots are compiled before they are stored, so this
    /// operation cannot create an invalid preview.
    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.definition, previous.clone());
        self.redo.push(current);
        self.preview = Level::from_def(previous).expect("editor history is always valid");
        self.selection = EditorSelection::None;
        self.last_error = None;
        true
    }

    /// Reapplies the next valid definition, returning whether anything was
    /// redone.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.definition, next.clone());
        self.undo.push(current);
        self.preview = Level::from_def(next).expect("editor history is always valid");
        self.selection = EditorSelection::None;
        self.last_error = None;
        true
    }

    fn select(&mut self, selection: EditorSelection) -> Result<EditorSelection, LevelEditorError> {
        validate_selection(&self.definition, selection)?;
        self.selection = selection;
        Ok(selection)
    }

    fn push_undo(&mut self, definition: LevelDef) {
        if self.undo.len() == MAX_UNDO_STEPS {
            self.undo.remove(0);
        }
        self.undo.push(definition);
    }
}

fn validate_selection(
    definition: &LevelDef,
    selection: EditorSelection,
) -> Result<(), LevelEditorError> {
    let valid = match selection {
        EditorSelection::None | EditorSelection::Spawn | EditorSelection::Bounds => true,
        EditorSelection::Solid(index) => index < definition.solids.len(),
        EditorSelection::OneWay(index) => index < definition.one_ways.len(),
        EditorSelection::Mover(index) => index < definition.movers.len(),
        EditorSelection::Slope(index) => index < definition.slopes.len(),
        EditorSelection::Enemy(index) => index < definition.enemies.len(),
        EditorSelection::Hazard(index) => index < definition.hazards.len(),
        EditorSelection::Pickup(index) => index < definition.pickups.len(),
        EditorSelection::Water(index) => index < definition.water.len(),
        EditorSelection::Ambience(index) => index < definition.ambience.len(),
        EditorSelection::PowerUp(index) => index < definition.powerups.len(),
        EditorSelection::Boss => definition.boss.is_some(),
        EditorSelection::Checkpoint(index) => index < definition.checkpoints.len(),
    };
    valid
        .then_some(())
        .ok_or(LevelEditorError::InvalidSelection(selection))
}

fn sanitize_preview_time(time: f32) -> f32 {
    if time.is_finite() {
        time.max(0.0)
    } else {
        0.0
    }
}

fn apply_mutation(
    definition: &mut LevelDef,
    selection: EditorSelection,
    command: EditorCommand,
) -> Result<EditorSelection, LevelEditorError> {
    match command {
        EditorCommand::Add(element) => Ok(insert_element(definition, element)),
        EditorCommand::Delete => delete_selection(definition, selection),
        EditorCommand::Translate(delta) => {
            validate_selection(definition, selection)?;
            translate_selection(definition, selection, delta)?;
            Ok(selection)
        }
        EditorCommand::Resize(delta) => {
            validate_selection(definition, selection)?;
            resize_selection(definition, selection, delta)?;
            Ok(selection)
        }
        EditorCommand::Select(_) => unreachable!("selection commands are handled before mutation"),
    }
}

fn translate_rect(rect: &mut RectDef, delta: Vec2) {
    rect.x += delta.x;
    rect.y += delta.y;
}

fn translate_selection(
    definition: &mut LevelDef,
    selection: EditorSelection,
    delta: Vec2,
) -> Result<(), LevelEditorError> {
    match selection {
        EditorSelection::None => return Err(LevelEditorError::NoSelection),
        EditorSelection::Spawn => translate_rect(&mut definition.spawn, delta),
        EditorSelection::Bounds => {
            definition.bounds.min_x += delta.x;
            definition.bounds.max_x += delta.x;
            definition.bounds.min_y += delta.y;
            definition.bounds.max_y += delta.y;
        }
        EditorSelection::Solid(index) => translate_rect(&mut definition.solids[index], delta),
        EditorSelection::OneWay(index) => translate_rect(&mut definition.one_ways[index], delta),
        EditorSelection::Mover(index) => translate_rect(&mut definition.movers[index].rect, delta),
        EditorSelection::Slope(index) => {
            let slope = &mut definition.slopes[index];
            translate_rect(&mut slope.rect, delta);
            slope.surface_left += delta.y;
            slope.surface_right += delta.y;
        }
        EditorSelection::Enemy(index) => {
            definition.enemies[index].x += delta.x;
            definition.enemies[index].y += delta.y;
        }
        EditorSelection::Hazard(index) => {
            translate_rect(&mut definition.hazards[index].rect, delta)
        }
        EditorSelection::Pickup(index) => {
            definition.pickups[index].x += delta.x;
            definition.pickups[index].y += delta.y;
        }
        EditorSelection::Water(index) => translate_rect(&mut definition.water[index], delta),
        EditorSelection::Ambience(index) => {
            translate_rect(&mut definition.ambience[index].rect, delta)
        }
        EditorSelection::PowerUp(index) => {
            definition.powerups[index].x += delta.x;
            definition.powerups[index].y += delta.y;
        }
        EditorSelection::Boss => {
            let boss = definition
                .boss
                .as_mut()
                .ok_or(LevelEditorError::InvalidSelection(selection))?;
            boss.x += delta.x;
            boss.y += delta.y;
        }
        EditorSelection::Checkpoint(index) => {
            definition.checkpoints[index].x += delta.x;
            definition.checkpoints[index].y += delta.y;
        }
    }
    Ok(())
}

fn resize_rect(rect: &mut RectDef, delta: Vec2) {
    rect.w += delta.x;
    rect.h += delta.y;
}

fn resize_selection(
    definition: &mut LevelDef,
    selection: EditorSelection,
    delta: Vec2,
) -> Result<(), LevelEditorError> {
    match selection {
        EditorSelection::None => return Err(LevelEditorError::NoSelection),
        EditorSelection::Spawn => resize_rect(&mut definition.spawn, delta),
        EditorSelection::Bounds => {
            definition.bounds.max_x += delta.x;
            definition.bounds.max_y += delta.y;
        }
        EditorSelection::Solid(index) => resize_rect(&mut definition.solids[index], delta),
        EditorSelection::OneWay(index) => resize_rect(&mut definition.one_ways[index], delta),
        EditorSelection::Mover(index) => resize_rect(&mut definition.movers[index].rect, delta),
        EditorSelection::Slope(index) => resize_rect(&mut definition.slopes[index].rect, delta),
        EditorSelection::Enemy(index) => definition.enemies[index].size += delta.x,
        EditorSelection::Hazard(index) => resize_rect(&mut definition.hazards[index].rect, delta),
        EditorSelection::Pickup(index) => definition.pickups[index].radius += delta.x,
        EditorSelection::Water(index) => resize_rect(&mut definition.water[index], delta),
        EditorSelection::Ambience(index) => {
            resize_rect(&mut definition.ambience[index].rect, delta)
        }
        EditorSelection::PowerUp(_) | EditorSelection::Checkpoint(_) => {
            return Err(LevelEditorError::CannotResize(selection))
        }
        EditorSelection::Boss => {
            let boss = definition
                .boss
                .as_mut()
                .ok_or(LevelEditorError::InvalidSelection(selection))?;
            boss.size += delta.x;
        }
    }
    Ok(())
}

fn delete_selection(
    definition: &mut LevelDef,
    selection: EditorSelection,
) -> Result<EditorSelection, LevelEditorError> {
    match selection {
        EditorSelection::None => Err(LevelEditorError::NoSelection),
        EditorSelection::Spawn | EditorSelection::Bounds => {
            Err(LevelEditorError::ProtectedSelection(selection))
        }
        EditorSelection::Solid(index) => {
            validate_selection(definition, selection)?;
            definition.solids.remove(index);
            Ok(EditorSelection::None)
        }
        EditorSelection::OneWay(index) => {
            validate_selection(definition, selection)?;
            definition.one_ways.remove(index);
            Ok(EditorSelection::None)
        }
        EditorSelection::Mover(index) => {
            validate_selection(definition, selection)?;
            definition.movers.remove(index);
            Ok(EditorSelection::None)
        }
        EditorSelection::Slope(index) => {
            validate_selection(definition, selection)?;
            definition.slopes.remove(index);
            Ok(EditorSelection::None)
        }
        EditorSelection::Enemy(index) => {
            validate_selection(definition, selection)?;
            definition.enemies.remove(index);
            Ok(EditorSelection::None)
        }
        EditorSelection::Hazard(index) => {
            validate_selection(definition, selection)?;
            definition.hazards.remove(index);
            Ok(EditorSelection::None)
        }
        EditorSelection::Pickup(index) => {
            validate_selection(definition, selection)?;
            definition.pickups.remove(index);
            Ok(EditorSelection::None)
        }
        EditorSelection::Water(index) => {
            validate_selection(definition, selection)?;
            definition.water.remove(index);
            Ok(EditorSelection::None)
        }
        EditorSelection::Ambience(index) => {
            validate_selection(definition, selection)?;
            definition.ambience.remove(index);
            Ok(EditorSelection::None)
        }
        EditorSelection::PowerUp(index) => {
            validate_selection(definition, selection)?;
            definition.powerups.remove(index);
            Ok(EditorSelection::None)
        }
        EditorSelection::Boss => {
            validate_selection(definition, selection)?;
            definition.boss = None;
            Ok(EditorSelection::None)
        }
        EditorSelection::Checkpoint(index) => {
            validate_selection(definition, selection)?;
            definition.checkpoints.remove(index);
            Ok(EditorSelection::None)
        }
    }
}

fn insert_element(definition: &mut LevelDef, element: LevelElement) -> EditorSelection {
    match element {
        LevelElement::Solid(value) => {
            definition.solids.push(value);
            EditorSelection::Solid(definition.solids.len() - 1)
        }
        LevelElement::OneWay(value) => {
            definition.one_ways.push(value);
            EditorSelection::OneWay(definition.one_ways.len() - 1)
        }
        LevelElement::Mover(value) => {
            definition.movers.push(value);
            EditorSelection::Mover(definition.movers.len() - 1)
        }
        LevelElement::Slope(value) => {
            definition.slopes.push(value);
            EditorSelection::Slope(definition.slopes.len() - 1)
        }
        LevelElement::Enemy(value) => {
            definition.enemies.push(value);
            EditorSelection::Enemy(definition.enemies.len() - 1)
        }
        LevelElement::Hazard(value) => {
            definition.hazards.push(value);
            EditorSelection::Hazard(definition.hazards.len() - 1)
        }
        LevelElement::Pickup(value) => {
            definition.pickups.push(value);
            EditorSelection::Pickup(definition.pickups.len() - 1)
        }
        LevelElement::Water(value) => {
            definition.water.push(value);
            EditorSelection::Water(definition.water.len() - 1)
        }
        LevelElement::Ambience(value) => {
            definition.ambience.push(value);
            EditorSelection::Ambience(definition.ambience.len() - 1)
        }
        LevelElement::PowerUp(value) => {
            definition.powerups.push(value);
            EditorSelection::PowerUp(definition.powerups.len() - 1)
        }
        LevelElement::Boss(value) => {
            definition.boss = Some(value);
            EditorSelection::Boss
        }
        LevelElement::Checkpoint(value) => {
            definition.checkpoints.push(value);
            EditorSelection::Checkpoint(definition.checkpoints.len() - 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{BoundsDef, PlayerTuning, ThemeDef};

    fn valid_definition() -> LevelDef {
        LevelDef {
            id: "editor-fixture".to_owned(),
            name: "EDITOR FIXTURE".to_owned(),
            gravity: 1_900.0,
            spawn: RectDef::centered(-100.0, 60.0, 44.0, 56.0),
            bounds: BoundsDef {
                min_x: -500.0,
                min_y: -500.0,
                max_x: 500.0,
                max_y: 500.0,
            },
            solids: vec![RectDef::centered(0.0, -80.0, 600.0, 160.0)],
            one_ways: Vec::new(),
            movers: Vec::new(),
            slopes: Vec::new(),
            enemies: Vec::new(),
            hazards: Vec::new(),
            pickups: vec![PickupDef {
                x: 100.0,
                y: 120.0,
                radius: 30.0,
            }],
            water: Vec::new(),
            ambience: Vec::new(),
            powerups: Vec::new(),
            boss: None,
            theme: Some(ThemeDef::default()),
            kill_y: -400.0,
            checkpoints: Vec::new(),
            solution_route: Vec::new(),
            player: PlayerTuning::default(),
        }
    }

    fn editor() -> LevelEditor {
        LevelEditor::new(valid_definition()).expect("fixture is valid")
    }

    #[test]
    fn translation_updates_definition_and_compiled_preview_together() {
        let mut editor = editor();
        editor
            .apply(EditorCommand::Select(EditorSelection::Solid(0)))
            .unwrap();
        editor
            .apply(EditorCommand::Translate(Vec2::new(25.0, 10.0)))
            .unwrap();

        assert_eq!(editor.definition().solids[0].x, 25.0);
        assert_eq!(editor.definition().solids[0].y, -70.0);
        assert_eq!(editor.preview().solids[0].center(), Vec2::new(25.0, -70.0));
        assert!(editor.is_dirty());
        assert!(editor.last_error().is_none());
    }

    #[test]
    fn invalid_geometry_is_rejected_without_partial_mutation() {
        let mut editor = editor();
        editor
            .apply(EditorCommand::Select(EditorSelection::Solid(0)))
            .unwrap();
        let before = editor.definition().clone();
        let preview_before = editor.preview().clone();

        let error = editor
            .apply(EditorCommand::Resize(Vec2::new(-700.0, 0.0)))
            .expect_err("negative width must be rejected");
        assert!(matches!(
            error,
            LevelEditorError::Validation(LevelError::NonPositiveSize(_))
        ));
        assert_eq!(editor.definition(), &before);
        assert_eq!(editor.preview().solids, preview_before.solids);
        assert_eq!(editor.last_error(), Some(&error));
        assert!(!editor.can_undo());
    }

    #[test]
    fn add_delete_undo_and_redo_keep_the_preview_compilable() {
        let mut editor = editor();
        editor
            .apply(EditorCommand::Add(LevelElement::Pickup(PickupDef {
                x: 220.0,
                y: 120.0,
                radius: 24.0,
            })))
            .unwrap();
        assert_eq!(editor.selection(), EditorSelection::Pickup(1));
        assert_eq!(editor.preview().pickups.len(), 2);

        editor.apply(EditorCommand::Delete).unwrap();
        assert_eq!(editor.selection(), EditorSelection::None);
        assert_eq!(editor.preview().pickups.len(), 1);
        assert!(editor.undo());
        assert_eq!(editor.preview().pickups.len(), 2);
        assert!(editor.redo());
        assert_eq!(editor.preview().pickups.len(), 1);
        assert!(!editor.redo());
    }

    #[test]
    fn saved_marker_and_json_export_follow_the_valid_definition() {
        let mut editor = editor();
        editor
            .apply(EditorCommand::Select(EditorSelection::Pickup(0)))
            .unwrap();
        editor
            .apply(EditorCommand::Translate(Vec2::new(20.0, 0.0)))
            .unwrap();
        assert!(editor.is_dirty());
        let json = editor.to_json().unwrap();
        let round_trip = LevelEditor::from_json(&json).unwrap();
        assert_eq!(round_trip.definition(), editor.definition());
        editor.mark_saved();
        assert!(!editor.is_dirty());
    }

    #[test]
    fn invalid_or_protected_selections_are_reported_without_panics() {
        let mut editor = editor();
        let error = editor
            .apply(EditorCommand::Select(EditorSelection::Solid(99)))
            .expect_err("missing item should be rejected");
        assert_eq!(
            error,
            LevelEditorError::InvalidSelection(EditorSelection::Solid(99))
        );
        let error = editor
            .apply(EditorCommand::Delete)
            .expect_err("there is no active selection");
        assert_eq!(error, LevelEditorError::NoSelection);
        editor
            .apply(EditorCommand::Select(EditorSelection::Spawn))
            .unwrap();
        let error = editor
            .apply(EditorCommand::Delete)
            .expect_err("spawn is structural");
        assert_eq!(
            error,
            LevelEditorError::ProtectedSelection(EditorSelection::Spawn)
        );

        editor
            .apply(EditorCommand::Add(LevelElement::PowerUp(PowerUpDef {
                x: 80.0,
                y: 120.0,
                kind: crate::level::PowerKind::DoubleJump,
            })))
            .unwrap();
        let error = editor
            .apply(EditorCommand::Resize(Vec2::new(1.0, 1.0)))
            .expect_err("power-up markers have no authored size");
        assert_eq!(
            error,
            LevelEditorError::CannotResize(EditorSelection::PowerUp(0))
        );
    }

    #[test]
    fn selected_bounds_are_available_for_static_and_dynamic_preview_outlines() {
        let mut editor = editor();
        editor
            .apply(EditorCommand::Select(EditorSelection::Solid(0)))
            .unwrap();
        assert_eq!(
            editor.selection_bounds_at(f32::NAN).unwrap().center(),
            Vec2::new(0.0, -80.0)
        );

        editor
            .apply(EditorCommand::Add(LevelElement::Mover(MoverDef {
                rect: RectDef::centered(200.0, 40.0, 80.0, 20.0),
                amplitude: 25.0,
                speed: 2.0,
                vertical: false,
                phase: 0.0,
            })))
            .unwrap();
        let at_start = editor.selection_bounds_at(0.0).unwrap().center();
        let later = editor.selection_bounds_at(0.5).unwrap().center();
        assert_eq!(at_start, Vec2::new(200.0, 40.0));
        assert_ne!(later, at_start);
    }
}
