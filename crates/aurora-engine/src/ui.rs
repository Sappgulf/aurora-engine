//! Small, renderer-agnostic game-flow and bitmap UI primitives.
//!
//! Games own their content and call [`MenuState::handle`] with semantic input;
//! renderers can turn [`BitmapText::glyphs`] into sprites, DOM nodes, or native UI.

use glam::Vec2;

use crate::profile::EngineProfile;

/// High-level screens shared by title, pause, settings, and results flows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuScreen {
    Main,
    HowTo,
    Settings,
    Pause,
    Results,
}

/// Whether a game is accepting simulation input or presenting a menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameFlow {
    Menu(MenuScreen),
    Playing,
}

/// Semantic menu intent, deliberately independent of keyboard or gamepad bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuInput {
    Up,
    Down,
    Confirm,
    Back,
}

#[derive(Debug, Clone, Copy)]
pub struct MenuNavigator {
    direction: Option<MenuInput>,
    elapsed: f32,
    initial_fired: bool,
    initial_delay: f32,
    repeat_interval: f32,
}

impl MenuNavigator {
    pub fn new(initial_delay: f32, repeat_interval: f32) -> Self {
        Self {
            direction: None,
            elapsed: 0.0,
            initial_fired: false,
            initial_delay: normalize_repeat_value(initial_delay, 0.35, 0.1),
            repeat_interval: normalize_repeat_value(repeat_interval, 0.1, 0.03),
        }
    }

    pub fn poll(&mut self, direction: Option<MenuInput>, delta: f32) -> Option<MenuInput> {
        let Some(direction) = direction else {
            self.reset();
            return None;
        };
        if !matches!(direction, MenuInput::Up | MenuInput::Down) {
            self.reset();
            return Some(direction);
        }
        if self.direction != Some(direction) {
            self.direction = Some(direction);
            self.elapsed = 0.0;
            return Some(direction);
        }

        self.elapsed += if delta.is_finite() && delta > 0.0 {
            delta
        } else {
            0.0
        };
        const REPEAT_EPSILON: f32 = 1.0e-5;
        if !self.initial_fired {
            if self.elapsed + REPEAT_EPSILON < self.initial_delay {
                return None;
            }
            self.elapsed -= self.initial_delay;
            self.initial_fired = true;
            return Some(direction);
        }

        let mut emitted = None;
        let mut repeats = 0;
        while self.elapsed + REPEAT_EPSILON >= self.repeat_interval && repeats < 8 {
            self.elapsed -= self.repeat_interval;
            repeats += 1;
            emitted = Some(direction);
        }
        emitted
    }

    pub fn reset(&mut self) {
        self.direction = None;
        self.elapsed = 0.0;
        self.initial_fired = false;
    }
}

/// Reversible settings editing for menus and platform-native settings views.
///
/// The engine commits the returned value only after [`Self::apply`]; previews
/// are deliberately isolated so Cancel can always restore the original
/// snapshot without trying to reverse individual field edits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SettingsTransaction {
    original: EngineProfile,
    previewed: EngineProfile,
}

impl SettingsTransaction {
    pub fn begin(profile: EngineProfile) -> Self {
        Self {
            original: profile,
            previewed: profile,
        }
    }

    pub fn preview(&mut self, profile: EngineProfile) {
        self.previewed = profile;
    }

    pub fn previewed(&self) -> EngineProfile {
        self.previewed
    }

    pub fn apply(self) -> EngineProfile {
        self.previewed.normalized()
    }

    pub fn cancel(self) -> EngineProfile {
        self.original
    }
}

fn normalize_repeat_value(value: f32, fallback: f32, minimum: f32) -> f32 {
    if value.is_finite() {
        value.max(minimum)
    } else {
        fallback.max(minimum)
    }
}

/// Intent returned by [`MenuState::handle`]. The host game performs the effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCommand {
    None,
    StartRun,
    RestartRun,
    Resume,
    Open(MenuScreen),
    ReturnToMain,
    TogglePostFx,
    ToggleReducedMotion,
    EndRun,
}

/// Stateful navigation for Aurora-style title, pause, settings, and results menus.
#[derive(Debug, Clone, Copy)]
pub struct MenuState {
    pub flow: GameFlow,
    selected: usize,
    pub post_fx: bool,
    pub reduced_motion: bool,
    settings_parent: MenuScreen,
}

impl Default for MenuState {
    fn default() -> Self {
        Self::new()
    }
}

impl MenuState {
    pub fn new() -> Self {
        Self {
            flow: GameFlow::Menu(MenuScreen::Main),
            selected: 0,
            post_fx: true,
            reduced_motion: false,
            settings_parent: MenuScreen::Main,
        }
    }

    pub fn screen(&self) -> Option<MenuScreen> {
        match self.flow {
            GameFlow::Menu(screen) => Some(screen),
            GameFlow::Playing => None,
        }
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn open(&mut self, screen: MenuScreen) {
        self.flow = GameFlow::Menu(screen);
        self.selected = 0;
    }

    pub fn play(&mut self) {
        self.flow = GameFlow::Playing;
        self.selected = 0;
    }

    pub fn handle(&mut self, input: MenuInput) -> MenuCommand {
        let Some(screen) = self.screen() else {
            if input == MenuInput::Back {
                self.open(MenuScreen::Pause);
                return MenuCommand::Open(MenuScreen::Pause);
            }
            return MenuCommand::None;
        };
        let count = item_count(screen);
        match input {
            MenuInput::Up => self.selected = (self.selected + count - 1) % count,
            MenuInput::Down => self.selected = (self.selected + 1) % count,
            MenuInput::Back => return self.back(screen),
            MenuInput::Confirm => return self.confirm(screen),
        }
        MenuCommand::None
    }

    fn back(&mut self, screen: MenuScreen) -> MenuCommand {
        match screen {
            MenuScreen::Pause => {
                self.play();
                MenuCommand::Resume
            }
            MenuScreen::Main => MenuCommand::None,
            MenuScreen::Settings => {
                let parent = self.settings_parent;
                self.open(parent);
                if parent == MenuScreen::Pause {
                    MenuCommand::Open(MenuScreen::Pause)
                } else {
                    MenuCommand::ReturnToMain
                }
            }
            MenuScreen::HowTo | MenuScreen::Results => {
                self.open(MenuScreen::Main);
                MenuCommand::ReturnToMain
            }
        }
    }

    fn confirm(&mut self, screen: MenuScreen) -> MenuCommand {
        match (screen, self.selected) {
            (MenuScreen::Main, 0) => {
                self.play();
                MenuCommand::StartRun
            }
            (MenuScreen::Main, 1) => {
                self.open(MenuScreen::HowTo);
                MenuCommand::Open(MenuScreen::HowTo)
            }
            (MenuScreen::Main, _) => {
                self.settings_parent = MenuScreen::Main;
                self.open(MenuScreen::Settings);
                MenuCommand::Open(MenuScreen::Settings)
            }
            (MenuScreen::Pause, 0) => {
                self.play();
                MenuCommand::Resume
            }
            (MenuScreen::Pause, 1) => {
                self.play();
                MenuCommand::RestartRun
            }
            (MenuScreen::Pause, 2) => {
                self.settings_parent = MenuScreen::Pause;
                self.open(MenuScreen::Settings);
                MenuCommand::Open(MenuScreen::Settings)
            }
            (MenuScreen::Pause, _) => {
                self.open(MenuScreen::Main);
                MenuCommand::EndRun
            }
            (MenuScreen::Settings, 0) => {
                self.post_fx = !self.post_fx;
                MenuCommand::TogglePostFx
            }
            (MenuScreen::Settings, 1) => {
                self.reduced_motion = !self.reduced_motion;
                MenuCommand::ToggleReducedMotion
            }
            (MenuScreen::Settings, _) => {
                let parent = self.settings_parent;
                self.open(parent);
                if parent == MenuScreen::Pause {
                    MenuCommand::Open(MenuScreen::Pause)
                } else {
                    MenuCommand::ReturnToMain
                }
            }
            (MenuScreen::Results, 0) => {
                self.play();
                MenuCommand::RestartRun
            }
            (MenuScreen::Results, _) => {
                self.open(MenuScreen::Main);
                MenuCommand::ReturnToMain
            }
            (MenuScreen::HowTo, _) => {
                self.open(MenuScreen::Main);
                MenuCommand::ReturnToMain
            }
        }
    }
}

fn item_count(screen: MenuScreen) -> usize {
    match screen {
        MenuScreen::Main => 3,
        MenuScreen::HowTo => 1,
        MenuScreen::Settings => 3,
        MenuScreen::Pause => 4,
        MenuScreen::Results => 2,
    }
}

/// One lit cell in a 5×7 bitmap glyph. Rendering remains backend-agnostic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphCell {
    pub position: Vec2,
    pub size: f32,
}

/// Compact uppercase bitmap font for game overlays and debug text.
pub struct BitmapText;

impl BitmapText {
    /// Returns lit 5×7 cells for `text`, positioned from its top-left corner.
    pub fn glyphs(text: &str, origin: Vec2, pixel_size: f32) -> Vec<GlyphCell> {
        let mut cells = Vec::new();
        let mut cursor = origin;
        for ch in text.chars() {
            if ch == '\n' {
                cursor.x = origin.x;
                cursor.y -= pixel_size * 9.0;
                continue;
            }
            if let Some(rows) = glyph(ch) {
                for (row, bits) in rows.iter().enumerate() {
                    for column in 0..5 {
                        if bits & (1 << (4 - column)) != 0 {
                            cells.push(GlyphCell {
                                position: cursor
                                    + Vec2::new(
                                        column as f32 * pixel_size,
                                        -(row as f32) * pixel_size,
                                    ),
                                size: pixel_size,
                            });
                        }
                    }
                }
            }
            cursor.x += pixel_size * 6.0;
        }
        cells
    }
}

fn glyph(ch: char) -> Option<[u8; 7]> {
    let ch = ch.to_ascii_uppercase();
    let rows = match ch {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b11100,
        ],
        ':' => [0, 0b00100, 0b00100, 0, 0b00100, 0b00100, 0],
        '-' => [0, 0, 0, 0b11111, 0, 0, 0],
        '/' => [0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0, 0],
        '.' => [0, 0, 0, 0, 0, 0b00100, 0b00100],
        '>' => [
            0b10000, 0b01000, 0b00100, 0b00010, 0b00100, 0b01000, 0b10000,
        ],
        ' ' => [0; 7],
        _ => return None,
    };
    Some(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_to_play_and_pause_round_trip() {
        let mut menu = MenuState::new();
        assert_eq!(menu.handle(MenuInput::Confirm), MenuCommand::StartRun);
        assert_eq!(menu.flow, GameFlow::Playing);
        assert_eq!(
            menu.handle(MenuInput::Back),
            MenuCommand::Open(MenuScreen::Pause)
        );
        assert_eq!(menu.handle(MenuInput::Confirm), MenuCommand::Resume);
        assert_eq!(menu.flow, GameFlow::Playing);
    }

    #[test]
    fn pause_can_restart_or_end_run() {
        let mut menu = MenuState::new();
        menu.open(MenuScreen::Pause);
        menu.handle(MenuInput::Down);
        assert_eq!(menu.handle(MenuInput::Confirm), MenuCommand::RestartRun);
        menu.open(MenuScreen::Pause);
        for _ in 0..3 {
            menu.handle(MenuInput::Down);
        }
        assert_eq!(menu.handle(MenuInput::Confirm), MenuCommand::EndRun);
        assert_eq!(menu.screen(), Some(MenuScreen::Main));
    }

    #[test]
    fn glyph_layout_handles_newline_and_unknown_glyphs() {
        let cells = BitmapText::glyphs("A\n?", Vec2::ZERO, 2.0);
        assert!(!cells.is_empty());
        assert!(cells.iter().all(|cell| cell.position.y <= 0.0));
    }

    #[test]
    fn menu_navigation_repeats_after_initial_delay() {
        let mut navigator = MenuNavigator::new(0.35, 0.10);
        assert_eq!(
            navigator.poll(Some(MenuInput::Down), 0.0),
            Some(MenuInput::Down)
        );
        assert_eq!(navigator.poll(Some(MenuInput::Down), 0.34), None);
        assert_eq!(
            navigator.poll(Some(MenuInput::Down), 0.01),
            Some(MenuInput::Down)
        );
        assert_eq!(navigator.poll(Some(MenuInput::Down), 0.09), None);
        assert_eq!(
            navigator.poll(Some(MenuInput::Down), 0.01),
            Some(MenuInput::Down)
        );
    }

    #[test]
    fn changing_direction_and_release_reset_repeat_state() {
        let mut navigator = MenuNavigator::new(0.35, 0.10);
        assert_eq!(
            navigator.poll(Some(MenuInput::Down), 0.0),
            Some(MenuInput::Down)
        );
        assert_eq!(
            navigator.poll(Some(MenuInput::Up), 0.0),
            Some(MenuInput::Up)
        );
        assert_eq!(navigator.poll(None, 1.0), None);
        assert_eq!(
            navigator.poll(Some(MenuInput::Up), 0.0),
            Some(MenuInput::Up)
        );
    }

    #[test]
    fn settings_transaction_supports_preview_apply_and_cancel() {
        let original = EngineProfile::default();
        let mut edited = original;
        edited.display.render_scale = 0.25;
        edited.accessibility.text_scale = 1.5;

        let mut transaction = SettingsTransaction::begin(original);
        transaction.preview(edited);
        assert_eq!(transaction.previewed(), edited);
        assert_eq!(transaction.cancel(), original);

        let mut transaction = SettingsTransaction::begin(original);
        transaction.preview(edited);
        let applied = transaction.apply();
        assert_eq!(applied.display.render_scale, 0.5);
        assert_eq!(applied.accessibility.text_scale, 1.5);
    }
}
