//! Data-driven mission definitions for the Last Light campaign.
//!
//! Keeping map layout, spawns, and victory conditions as plain data (rather
//! than hardcoded in `main.rs`) is what lets a second mission reuse the same
//! simulation/rendering code instead of forking it.

use aurora_engine::{Aabb, TerrainZone};
use glam::Vec2;

use crate::assets::TextureAsset;
use crate::mission_state::{
    EngineerRepairObjective, ResourceObjective, SpecialistObjective, SpecialistObjectiveKind,
    TerrainControlObjective,
};
use crate::units::UnitKind;

/// World-space size shared with Last Light's tactical camera and NavGrid.
/// Mission coordinates are authored against this rectangle so a future
/// map-scale change can be audited in one place instead of clipping a
/// resource node or objective at runtime.
#[allow(dead_code)]
pub const PLAYFIELD_SIZE: Vec2 = Vec2::new(3_600.0, 2_200.0);

/// Minimum spacing between resource anchors. A unit footprint is 64 world
/// units; this wider guard keeps two nodes from collapsing into one worker
/// route and preserves the authored safe/contested/distant triangle.
pub const MIN_RESOURCE_NODE_SEPARATION: f32 = 128.0;

#[derive(Debug, Clone, Copy)]
pub struct PlayerSpawn {
    pub kind: UnitKind,
    pub position: Vec2,
    pub health: f32,
    pub speed: f32,
    pub escort: bool,
    pub callsign: Option<&'static str>,
}

impl PlayerSpawn {
    pub fn new(kind: UnitKind, position: Vec2, health: f32, speed: f32) -> Self {
        Self {
            kind,
            position,
            health,
            speed,
            escort: false,
            callsign: None,
        }
    }

    /// Gives a campaign-critical field unit a durable identity. Production
    /// units remain anonymous until a later roster system assigns a callsign.
    pub const fn named(mut self, callsign: &'static str) -> Self {
        self.callsign = Some(callsign);
        self
    }

    /// Marks this spawn as the unit an `EscortToExtraction` victory
    /// condition tracks — defeat triggers if it dies.
    pub fn escort(mut self) -> Self {
        self.escort = true;
        self
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EnemySpawn {
    pub kind: UnitKind,
    pub position: Vec2,
    pub health: f32,
    pub speed: f32,
}

impl EnemySpawn {
    pub fn new(kind: UnitKind, position: Vec2, health: f32, speed: f32) -> Self {
        Self {
            kind,
            position,
            health,
            speed,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum VictoryCondition {
    /// Restore every relay and defeat the named boss unit kind.
    RestoreRelaysAndDefeatBoss { boss_kind: UnitKind },
    /// Get the mission's escort unit alive within `radius` of `point`.
    EscortToExtraction { point: Vec2, radius: f32 },
}

#[derive(Debug, Clone, Copy)]
pub enum DialogueTrigger {
    Time(f32),
    RelaysOnline(usize),
    SalvageDelivered(u32),
    EnemyRaid(u32),
    UnitDestroyed(UnitKind),
    /// Fires once a mission-authored Surveyor/Warden resource contract has
    /// finished. The renderer consumes this through the normal radio queue;
    /// the trigger stays data-only so headless campaign traces can replay it.
    ResourceObjectiveCompleted,
}

#[derive(Debug, Clone, Copy)]
pub struct RadioLine {
    pub speaker: &'static str,
    pub text: &'static str,
    pub trigger: DialogueTrigger,
}

#[derive(Debug, Clone)]
pub struct MissionDef {
    pub id: &'static str,
    pub title: &'static str,
    pub briefing_story: &'static str,
    pub victory_title: &'static str,
    pub victory_story: &'static str,
    pub defeat_title: &'static str,
    pub defeat_story: &'static str,
    pub relays: Vec<Vec2>,
    pub salvage_nodes: Vec<Vec2>,
    pub radio_lines: Vec<RadioLine>,
    pub reactor_position: Option<Vec2>,
    pub fabricator_position: Vec2,
    pub player_spawns: Vec<PlayerSpawn>,
    pub enemy_spawns: Vec<EnemySpawn>,
    /// Static blockers used to build the mission's `NavGrid` (corridor
    /// walls, etc.) and to obstruct placement.
    pub obstacles: Vec<Aabb>,
    /// Authored elevation and cover zones consumed by combat.
    pub terrain_zones: Vec<TerrainZone>,
    /// Position of the "wake Lumen" interaction, if this mission has one.
    pub lumen_console: Option<Vec2>,
    /// Optional role-specific campaign beat. This is separate from victory
    /// so a mission can teach a specialist job without changing its generic
    /// relay/escort completion contract.
    pub specialist_objective: Option<SpecialistObjective>,
    /// Separate Engineer contract with its own role copy and target geometry;
    /// the runtime still presents it through the shared objective HUD/state.
    pub engineer_repair_objective: Option<EngineerRepairObjective>,
    /// Optional terrain-control beat. Unlike a generic role objective, this
    /// contract requires the named unit to occupy the resolved high-ground or
    /// cover band at its target while the player clears any contest.
    pub terrain_control_objective: Option<TerrainControlObjective>,
    /// Optional finite-node objective. Unlike the generic harvest loop, this
    /// contract asks the authored worker to secure one strategic pocket while
    /// a support role can clear a live enemy contest.
    pub resource_objective: Option<ResourceObjective>,
    pub victory: VictoryCondition,
    pub unlock_next: u32,
    pub reward_lumen: u64,
    /// Campaign decision recorded automatically on victory (independent of
    /// any mid-mission choice like `lumen_console`).
    pub unlock_decision: Option<&'static str>,
    /// Mission-specific environment plate used by the renderer.
    pub environment_plate: TextureAsset,
    /// Minimum `CampaignProgress::unlocked_mission` tier required to select
    /// this mission from the mission-select screen.
    pub required_tier: u32,
}

impl MissionDef {
    /// Returns the fixed tactical playfield used by the native and browser
    /// renderers. This is intentionally a function (rather than a second
    /// `Aabb` constant) so callers cannot mutate a shared bounds value.
    #[allow(dead_code)]
    pub fn playfield_bounds() -> Aabb {
        Aabb::from_center_size(Vec2::ZERO, PLAYFIELD_SIZE)
    }

    /// Validate authored map data before it reaches the runtime.
    ///
    /// This catches the class of content bugs that are otherwise hard to
    /// diagnose in playtests: a spawn embedded in a blocker, a terrain band
    /// outside the camera, or a malformed cover value that changes combat
    /// math. The method is deliberately independent from rendering and
    /// simulation so it can run in asset CI and editor tooling later.
    #[allow(dead_code)]
    pub fn validate_layout(&self) -> Result<(), &'static str> {
        let bounds = Self::playfield_bounds();
        if self.salvage_nodes.len() < 3 {
            return Err("mission needs at least three resource nodes");
        }
        for (index, position) in self.salvage_nodes.iter().enumerate() {
            for other in self.salvage_nodes.iter().take(index) {
                if position.distance(*other) < MIN_RESOURCE_NODE_SEPARATION {
                    return Err("resource nodes are too close for distinct worker routes");
                }
            }
        }
        let mut anchors = Vec::with_capacity(
            self.relays.len()
                + self.salvage_nodes.len()
                + self.player_spawns.len()
                + self.enemy_spawns.len()
                + self.terrain_zones.len()
                + 2,
        );
        anchors.extend(self.relays.iter().copied());
        anchors.extend(self.salvage_nodes.iter().copied());
        anchors.extend(self.player_spawns.iter().map(|spawn| spawn.position));
        anchors.extend(self.enemy_spawns.iter().map(|spawn| spawn.position));
        anchors.push(self.fabricator_position);
        if let Some(position) = self.reactor_position {
            anchors.push(position);
        }
        if let VictoryCondition::EscortToExtraction { point, radius } = self.victory {
            if !radius.is_finite() || radius <= 0.0 {
                return Err("escort radius must be finite and positive");
            }
            anchors.push(point);
        }
        if let Some(position) = self.lumen_console {
            anchors.push(position);
        }
        if let Some(objective) = self.specialist_objective {
            objective.validate()?;
            anchors.push(objective.target);
            if !self
                .player_spawns
                .iter()
                .any(|spawn| spawn.kind == objective.kind.required_unit())
            {
                return Err("specialist objective requires its unit role");
            }
            if !self
                .terrain_zones
                .iter()
                .any(|zone| zone.bounds.contains_point(objective.target))
            {
                return Err("specialist objective target needs authored terrain");
            }
        }
        if let Some(objective) = self.engineer_repair_objective {
            objective.validate()?;
            anchors.push(objective.target);
            if !self
                .player_spawns
                .iter()
                .any(|spawn| spawn.kind == objective.required_unit())
            {
                return Err("engineer repair objective requires an Engineer");
            }
            if !self
                .terrain_zones
                .iter()
                .any(|zone| zone.bounds.contains_point(objective.target))
            {
                return Err("engineer repair objective target needs authored terrain");
            }
        }
        if let Some(objective) = self.terrain_control_objective {
            objective.validate()?;
            anchors.push(objective.target);
            if !self
                .player_spawns
                .iter()
                .any(|spawn| spawn.kind == objective.required_unit)
            {
                return Err("terrain control objective requires its unit role");
            }
            let Some((_, zone)) = TerrainZone::resolve_at(objective.target, &self.terrain_zones)
            else {
                return Err("terrain control objective target needs authored terrain");
            };
            if !objective.terrain_satisfies(zone.elevation, zone.cover) {
                return Err("terrain control objective target needs matching terrain");
            }
        }
        if let Some(objective) = self.resource_objective {
            objective.validate()?;
            let Some(&target) = self.salvage_nodes.get(objective.node_index) else {
                return Err("resource objective node index is out of range");
            };
            anchors.push(target);
            if !self
                .player_spawns
                .iter()
                .any(|spawn| spawn.kind == objective.worker_kind)
            {
                return Err("resource objective requires its worker role");
            }
            if let Some(support_kind) = objective.support_kind {
                if !self
                    .player_spawns
                    .iter()
                    .any(|spawn| spawn.kind == support_kind)
                {
                    return Err("resource objective requires its support role");
                }
            }
            if !self
                .terrain_zones
                .iter()
                .any(|zone| zone.bounds.contains_point(target))
            {
                return Err("resource objective node needs authored terrain");
            }
        }

        if anchors
            .iter()
            .any(|position| !position.is_finite() || !bounds.contains_point(*position))
        {
            return Err("mission anchor lies outside the tactical playfield");
        }

        for obstacle in &self.obstacles {
            let size = obstacle.size();
            if !size.is_finite() || size.x <= 0.0 || size.y <= 0.0 {
                return Err("mission blocker must have positive finite size");
            }
            if !bounds.contains_point(obstacle.min) || !bounds.contains_point(obstacle.max) {
                return Err("mission blocker lies outside the tactical playfield");
            }
            if anchors
                .iter()
                .any(|position| obstacle.contains_point(*position))
            {
                return Err("mission blocker overlaps an authored anchor");
            }
        }

        for zone in &self.terrain_zones {
            let size = zone.bounds.size();
            if !size.is_finite() || size.x <= 0.0 || size.y <= 0.0 {
                return Err("terrain zone must have positive finite size");
            }
            if !bounds.contains_point(zone.bounds.min) || !bounds.contains_point(zone.bounds.max) {
                return Err("terrain zone lies outside the tactical playfield");
            }
            if !zone.cover.is_finite() || !(0.0..=0.3).contains(&zone.cover) {
                return Err("terrain cover must stay within the engine's 0..0.3 contract");
            }
        }

        Ok(())
    }

    /// Expands authored tactical coordinates while preserving mission logic.
    /// This keeps spawns, objectives, blockers, and extraction points in one
    /// transform instead of letting individual systems drift out of sync.
    fn expanded(mut self, scale: f32) -> Self {
        for position in &mut self.relays {
            *position *= scale;
        }
        for position in &mut self.salvage_nodes {
            *position *= scale;
        }
        self.reactor_position = self.reactor_position.map(|position| position * scale);
        self.fabricator_position *= scale;
        for spawn in &mut self.player_spawns {
            spawn.position *= scale;
        }
        for spawn in &mut self.enemy_spawns {
            spawn.position *= scale;
        }
        for obstacle in &mut self.obstacles {
            *obstacle = Aabb::new(obstacle.min * scale, obstacle.max * scale);
        }
        for zone in &mut self.terrain_zones {
            zone.bounds = Aabb::new(zone.bounds.min * scale, zone.bounds.max * scale);
        }
        self.lumen_console = self.lumen_console.map(|position| position * scale);
        if let Some(objective) = &mut self.specialist_objective {
            objective.target *= scale;
            objective.radius *= scale;
        }
        if let Some(objective) = &mut self.engineer_repair_objective {
            objective.target *= scale;
            objective.radius *= scale;
        }
        if let Some(objective) = &mut self.terrain_control_objective {
            objective.target *= scale;
            objective.radius *= scale;
        }
        if let Some(objective) = &mut self.resource_objective {
            objective.worker_radius *= scale;
            objective.support_radius *= scale;
            objective.contest_radius *= scale;
        }
        if let VictoryCondition::EscortToExtraction { point, .. } = &mut self.victory {
            *point *= scale;
        }
        self
    }
}

/// All missions in campaign order, for the mission-select screen.
pub fn all() -> Vec<MissionDef> {
    vec![
        reclaim_the_reactor(),
        voice_in_conduit_twelve(),
        terms_of_salvage(),
        garden_below(),
        choir_invisible(),
        vesper_gate(),
        hollow_orbit(),
    ]
}

pub fn reclaim_the_reactor() -> MissionDef {
    MissionDef {
        id: "reclaim-the-reactor",
        title: "RECLAIM THE REACTOR",
        briefing_story: "MARA VEY: FIND IVO. RESTORE THREE RELAYS. SILENCE THE CHOIR.",
        victory_title: "REACTOR ONLINE",
        victory_story: "LUMEN: I CAN SEE YOU NOW, COMMANDER.",
        defeat_title: "LANTERN LOST",
        defeat_story: "THE DARK CLOSES OVER CONDUIT TWELVE.",
        relays: vec![
            Vec2::new(-790.0, 320.0),
            Vec2::new(30.0, -430.0),
            Vec2::new(830.0, 250.0),
        ],
        salvage_nodes: vec![
            Vec2::new(-610.0, -180.0),
            Vec2::new(40.0, 260.0),
            Vec2::new(720.0, -180.0),
            Vec2::new(-1_520.0, -720.0),
            Vec2::new(1_480.0, 720.0),
        ],
        radio_lines: vec![
            RadioLine {
                speaker: "MARA VEY",
                text: "LANTERN TEAM, STAY TOGETHER. THE CHOIR HEARS ISOLATION.",
                trigger: DialogueTrigger::Time(2.0),
            },
            RadioLine {
                speaker: "IVO RENN",
                text: "SURVEYOR LOADS TWENTY-FOUR. KEEP ITS RETURN LANE TO THE FABRICATOR CLEAR.",
                trigger: DialogueTrigger::Time(9.0),
            },
            RadioLine {
                speaker: "MARA VEY",
                text: "WARDEN, TAKE RELAY ALPHA HIGHGROUND; HOLD COVER UNTIL THE CHOIR CLOSES.",
                trigger: DialogueTrigger::Time(15.0),
            },
            RadioLine {
                speaker: "IVO RENN",
                text: "ENGINEER, KEEP RELAY KITS IN REACH. BELL MINES WILL HIT HARD OUT OF COVER.",
                trigger: DialogueTrigger::Time(19.0),
            },
            RadioLine {
                speaker: "SENA QUILL",
                text: "SURVEYOR, TAKE NODE ONE AND ROUTE SALVAGE HOME. TWO MORE NODES STAY COLD.",
                trigger: DialogueTrigger::RelaysOnline(1),
            },
            RadioLine {
                speaker: "MARA VEY",
                text: "SECOND RELAY IS OUR RIDGE. TAKE THE COVER, THEN LET THE CHOIR COME TO US.",
                trigger: DialogueTrigger::RelaysOnline(2),
            },
            RadioLine {
                speaker: "LUMEN",
                text: "THREE LIGHTS. ONE VOICE. COME FIND ME BENEATH THE REACTOR.",
                trigger: DialogueTrigger::RelaysOnline(3),
            },
            RadioLine {
                speaker: "MARA VEY",
                text: "THE LATTICE IS OUR GROUND NOW. HOLD THE LIGHT AND BREAK ITS CONDUCTOR.",
                trigger: DialogueTrigger::Time(38.0),
            },
            RadioLine {
                speaker: "IVO ROOK",
                text: "CHOIR CREDITS ARE RISING. KEEP A WARDEN ON THE ACTIVE RELAY; CONTACT IS COMING.",
                trigger: DialogueTrigger::Time(52.0),
            },
            RadioLine {
                speaker: "IVO RENN",
                text: "SALVAGE RETURNED. THE FABRICATOR CAN BREATHE AGAIN.",
                trigger: DialogueTrigger::SalvageDelivered(24),
            },
            RadioLine {
                speaker: "SENA QUILL",
                text: "THE CHOIR HAS FUNDED A RAID. EXPECT CONTACT FROM THE DARK EDGE.",
                trigger: DialogueTrigger::EnemyRaid(1),
            },
        ],
        reactor_position: Some(Vec2::new(520.0, -40.0)),
        fabricator_position: Vec2::new(-1_020.0, -120.0),
        player_spawns: vec![
            PlayerSpawn::new(UnitKind::Warden, Vec2::new(-880.0, -290.0), 155.0, 175.0)
                .named("MARA VEY"),
            PlayerSpawn::new(UnitKind::Engineer, Vec2::new(-790.0, -350.0), 115.0, 150.0)
                .named("IVO ROOK"),
            PlayerSpawn::new(UnitKind::Surveyor, Vec2::new(-900.0, -410.0), 90.0, 215.0)
                .named("SENA QUILL"),
        ],
        enemy_spawns: vec![
            EnemySpawn::new(UnitKind::Needle, Vec2::new(-480.0, 250.0), 90.0, 125.0),
            EnemySpawn::new(UnitKind::BellMine, Vec2::new(-120.0, -330.0), 90.0, 75.0),
            EnemySpawn::new(UnitKind::Needle, Vec2::new(290.0, 290.0), 90.0, 125.0),
            EnemySpawn::new(UnitKind::BellMine, Vec2::new(650.0, -310.0), 90.0, 75.0),
            EnemySpawn::new(UnitKind::Needle, Vec2::new(930.0, 390.0), 90.0, 125.0),
            EnemySpawn::new(UnitKind::Canticle, Vec2::new(520.0, 40.0), 340.0, 125.0),
        ],
        obstacles: Vec::new(),
        terrain_zones: vec![
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(20.0, -250.0), Vec2::new(1_050.0, 420.0)),
                1,
                0.18,
            ),
            // The first salvage cache is the opening economy lesson. Give
            // the Surveyor a small, readable extraction pocket near the
            // Fabricator while the Warden moves toward the relay ridge; the
            // live simulation consumes this cover for combat and pathing.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(-610.0, -180.0), Vec2::new(300.0, 220.0)),
                0,
                0.24,
            ),
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(-790.0, 320.0), Vec2::new(360.0, 280.0)),
                0,
                0.28,
            ),
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(830.0, 250.0), Vec2::new(360.0, 280.0)),
                0,
                0.28,
            ),
            // The middle salvage pocket is a deliberate hold-or-flank
            // decision: a Surveyor can harvest under cover, but the Warden
            // must leave the ridge to contest the Choir approach.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(40.0, 260.0), Vec2::new(260.0, 210.0)),
                0,
                0.22,
            ),
            // A second firing perch protects the eastern Flux route without
            // making the reactor itself a permanent safe zone.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(720.0, -180.0), Vec2::new(300.0, 190.0)),
                1,
                0.12,
            ),
        ],
        lumen_console: None,
        specialist_objective: None,
        engineer_repair_objective: None,
        terrain_control_objective: None,
        resource_objective: None,
        victory: VictoryCondition::RestoreRelaysAndDefeatBoss {
            boss_kind: UnitKind::Canticle,
        },
        unlock_next: 3,
        reward_lumen: 80,
        unlock_decision: Some("lumen-contact-established"),
        environment_plate: TextureAsset::ReactorSectorReclaim,
        required_tier: 1,
    }
}

pub fn voice_in_conduit_twelve() -> MissionDef {
    let extraction = Vec2::new(980.0, -40.0);
    MissionDef {
        id: "voice-in-conduit-twelve",
        title: "A VOICE IN CONDUIT TWELVE",
        briefing_story:
            "SENA QUILL: SOMETHING IN THE MAINTENANCE SPINE IS AWAKE. ESCORT ME TO THE ARRAY.",
        victory_title: "SIGNAL CLEAR",
        victory_story: "SENA: IT KNOWS MY NAME. IT'S BEEN WAITING.",
        defeat_title: "SIGNAL LOST",
        defeat_story: "CONDUIT TWELVE GOES QUIET.",
        relays: Vec::new(),
        salvage_nodes: vec![
            Vec2::new(-640.0, 260.0),
            Vec2::new(80.0, 20.0),
            Vec2::new(720.0, 40.0),
            Vec2::new(-1_470.0, -680.0),
            Vec2::new(1_500.0, 660.0),
        ],
        radio_lines: vec![
            RadioLine {
                speaker: "SENA QUILL",
                text: "THE SIGNAL MOVES WHEN I MOVE. ESCORT ME THROUGH THE SPINE.",
                trigger: DialogueTrigger::Time(2.0),
            },
            RadioLine {
                speaker: "IVO RENN",
                text: "BELL MINES AHEAD. WARDENS FIRST, SURVEYOR WIDE.",
                trigger: DialogueTrigger::Time(10.0),
            },
            RadioLine {
                speaker: "SENA QUILL",
                text: "THE SPINE HAS A HIGHER DECK AHEAD. KEEP ME ON THE LIT SIDE OF THE RAMP.",
                trigger: DialogueTrigger::Time(16.0),
            },
            RadioLine {
                speaker: "MARA VEY",
                text:
                    "WARDEN, HOLD THE NARROW DECK. SURVEYOR, MOVE THE LAST NODE WHEN COVER OPENS.",
                trigger: DialogueTrigger::Time(20.0),
            },
            RadioLine {
                speaker: "LUMEN",
                text: "SENA QUILL. YOUR SIGNAL FITS THE EMPTY PLACE IN ME.",
                trigger: DialogueTrigger::Time(24.0),
            },
            RadioLine {
                speaker: "IVO RENN",
                text:
                    "ENGINEER, KEEP THE GROUP STACKED. REPAIR ANY DAMAGED GROUND COVERS ON THE WAY.",
                trigger: DialogueTrigger::Time(28.0),
            },
            RadioLine {
                speaker: "MARA VEY",
                text: "ARRAY IN SIGHT. NO HERO RUNS AHEAD OF THE FORMATION—WE ARRIVE TOGETHER.",
                trigger: DialogueTrigger::Time(34.0),
            },
        ],
        reactor_position: None,
        fabricator_position: Vec2::new(-1_100.0, 380.0),
        player_spawns: vec![
            PlayerSpawn::new(UnitKind::Warden, Vec2::new(-980.0, 300.0), 175.0, 175.0)
                .named("MARA VEY"),
            PlayerSpawn::new(UnitKind::Engineer, Vec2::new(-1_040.0, 250.0), 130.0, 150.0)
                .named("IVO ROOK"),
            PlayerSpawn::new(UnitKind::Surveyor, Vec2::new(-900.0, 340.0), 90.0, 210.0)
                .named("SENA QUILL")
                .escort(),
        ],
        enemy_spawns: vec![
            EnemySpawn::new(UnitKind::Needle, Vec2::new(-400.0, 260.0), 90.0, 125.0),
            // Keep the first Needle in the lower maintenance lane; the
            // previous point was embedded in the authored horizontal wall.
            EnemySpawn::new(UnitKind::Needle, Vec2::new(-100.0, -150.0), 90.0, 125.0),
            EnemySpawn::new(UnitKind::BellMine, Vec2::new(300.0, 200.0), 90.0, 75.0),
            EnemySpawn::new(UnitKind::Needle, Vec2::new(600.0, -120.0), 90.0, 125.0),
            EnemySpawn::new(UnitKind::Needle, Vec2::new(850.0, 150.0), 90.0, 125.0),
        ],
        obstacles: vec![
            Aabb::from_center_size(Vec2::new(-500.0, 480.0), Vec2::new(1_400.0, 90.0)),
            Aabb::from_center_size(Vec2::new(-500.0, -60.0), Vec2::new(1_400.0, 90.0)),
            Aabb::from_center_size(Vec2::new(250.0, 210.0), Vec2::new(90.0, 500.0)),
            Aabb::from_center_size(Vec2::new(650.0, -260.0), Vec2::new(90.0, 400.0)),
        ],
        terrain_zones: vec![
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(-460.0, 230.0), Vec2::new(900.0, 380.0)),
                1,
                0.22,
            ),
            // The middle cache is the escort map's deliberate work pocket:
            // the Surveyor can harvest under light cover while the Warden
            // holds the gate, instead of paying full open-lane damage for
            // taking the first resource detour.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(100.0, 20.0), Vec2::new(180.0, 180.0)),
                0,
                0.24,
            ),
            TerrainZone::new(
                Aabb::from_center_size(extraction, Vec2::new(300.0, 260.0)),
                0,
                0.26,
            ),
        ],
        lumen_console: Some(extraction),
        specialist_objective: Some(SpecialistObjective::new(
            SpecialistObjectiveKind::SurveyorScan,
            extraction,
            110.0,
            6.0,
        )),
        engineer_repair_objective: None,
        terrain_control_objective: None,
        resource_objective: None,
        victory: VictoryCondition::EscortToExtraction {
            point: extraction,
            radius: 140.0,
        },
        unlock_next: 4,
        reward_lumen: 90,
        unlock_decision: None,
        environment_plate: TextureAsset::ReactorSectorVoice,
        required_tier: 3,
    }
}

/// Mission four is intentionally built from the same mission contract as the
/// first two: authored spawns, power objectives, dialogue, and a game-owned
/// campaign consequence. It proves that campaign growth does not require a
/// bespoke screen or renderer branch for every mission.
pub fn terms_of_salvage() -> MissionDef {
    MissionDef {
        id: "terms-of-salvage",
        title: "TERMS OF SALVAGE",
        briefing_story: "PREFECT VALE: CLAIM THE VAULT RELAYS BEFORE THE CHOIR DOES. THEN WE TALK.",
        victory_title: "CHARTER SECURED",
        victory_story: "VALE: A TEMPORARY ACCORD. DO NOT MAKE ME REGRET IT.",
        defeat_title: "VAULT LOST",
        defeat_story: "THE COMPACT CLOSES THE DOORS. THE CHOIR KEEPS THE LIGHT.",
        relays: vec![Vec2::new(-540.0, 250.0), Vec2::new(610.0, -180.0)],
        salvage_nodes: vec![
            Vec2::new(-720.0, -260.0),
            Vec2::new(-100.0, 180.0),
            Vec2::new(770.0, 250.0),
            Vec2::new(260.0, -520.0),
            Vec2::new(1_030.0, -320.0),
        ],
        radio_lines: vec![
            RadioLine {
                speaker: "PREFECT VALE",
                text: "THE VAULT IS NOT YOURS. PROVE YOU CAN KEEP ITS POWER STABLE.",
                trigger: DialogueTrigger::Time(2.0),
            },
            RadioLine {
                speaker: "MARA VEY",
                text: "WE ARE NOT HERE TO OWN A LIGHT. WE ARE HERE TO KEEP IT ON.",
                trigger: DialogueTrigger::RelaysOnline(1),
            },
            RadioLine {
                speaker: "IVO ROOK",
                text: "ONE HOSTILE DOWN. KEEP THE SURVEYOR MOVING WHILE THE VAULT IS OPEN.",
                trigger: DialogueTrigger::UnitDestroyed(UnitKind::Needle),
            },
            RadioLine {
                speaker: "IVO ROOK",
                text: "SECOND RELAY IS LIVE. FABRICATOR HAS A CLEAN LINE TO THE VAULT.",
                trigger: DialogueTrigger::RelaysOnline(2),
            },
            RadioLine {
                speaker: "PREFECT VALE",
                text: "THE VAULT IS PAYING OUT. DO NOT WASTE ITS SALVAGE ON A PANIC BUILD.",
                trigger: DialogueTrigger::SalvageDelivered(24),
            },
            RadioLine {
                speaker: "MARA VEY",
                text: "CHOIR RAID ON THE EAST RELAY. KEEP THE ENGINEER WORKING AND THE WARDEN MOVING.",
                trigger: DialogueTrigger::EnemyRaid(1),
            },
        ],
        reactor_position: Some(Vec2::new(430.0, 10.0)),
        fabricator_position: Vec2::new(-1_030.0, -160.0),
        player_spawns: vec![
            PlayerSpawn::new(UnitKind::Warden, Vec2::new(-890.0, -260.0), 175.0, 175.0)
                .named("MARA VEY"),
            PlayerSpawn::new(UnitKind::Engineer, Vec2::new(-950.0, -340.0), 130.0, 150.0)
                .named("IVO ROOK"),
            PlayerSpawn::new(UnitKind::Surveyor, Vec2::new(-820.0, -390.0), 90.0, 210.0)
                .named("SENA QUILL"),
        ],
        enemy_spawns: vec![
            EnemySpawn::new(UnitKind::Needle, Vec2::new(-330.0, 300.0), 95.0, 130.0),
            EnemySpawn::new(UnitKind::BellMine, Vec2::new(-60.0, -260.0), 95.0, 80.0),
            EnemySpawn::new(UnitKind::Needle, Vec2::new(310.0, 220.0), 95.0, 130.0),
            EnemySpawn::new(UnitKind::Canticle, Vec2::new(560.0, 30.0), 380.0, 120.0),
        ],
        obstacles: vec![
            Aabb::from_center_size(
                Vec2::new(20.0, 480.0),
                Vec2::new(1_650.0, 80.0),
            ),
            // The vault's central support creates a readable L-shaped
            // blocker after expansion: push through the high-ground lane or
            // spend time flanking around its open end.
            Aabb::from_center_size(Vec2::new(420.0, 300.0), Vec2::new(80.0, 420.0)),
        ],
        terrain_zones: vec![
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(260.0, -170.0), Vec2::new(860.0, 360.0)),
                1,
                0.2,
            ),
            // The first cache is the safe-start economy lesson. Give the
            // Surveyor a defendable pocket near the Fabricator while the
            // Warden contests the vault ridge; the later nodes remain open
            // so the mission still asks whether to greed for Flux.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(-720.0, -260.0), Vec2::new(320.0, 240.0)),
                0,
                0.24,
            ),
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(-540.0, 250.0), Vec2::new(340.0, 300.0)),
                0,
                0.3,
            ),
            // The first relay is also a terrain-control beat: the Warden can
            // hold this narrow ridge for a clean firing angle, but the lower
            // covered apron remains a fallback while the Choir contests it.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(-540.0, 250.0), Vec2::new(280.0, 220.0)),
                1,
                0.14,
            ),
            // The reactor is the late-game hold point. Keep its surrounding
            // apron small enough to contest, but give the Engineer a real
            // firing pocket instead of placing the objective on a zone edge.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(430.0, 10.0), Vec2::new(300.0, 240.0)),
                0,
                0.22,
            ),
        ],
        lumen_console: None,
        specialist_objective: Some(SpecialistObjective::new(
            SpecialistObjectiveKind::WardenHold,
            Vec2::new(-540.0, 250.0),
            120.0,
            7.0,
        )),
        engineer_repair_objective: None,
        terrain_control_objective: Some(TerrainControlObjective::high_ground_hold(
            Vec2::new(-540.0, 250.0),
            105.0,
            UnitKind::Warden,
            6.0,
        )),
        resource_objective: None,
        victory: VictoryCondition::RestoreRelaysAndDefeatBoss {
            boss_kind: UnitKind::Canticle,
        },
        unlock_next: 5,
        reward_lumen: 100,
        unlock_decision: Some("meridian-allied"),
        environment_plate: TextureAsset::ReactorSectorTerms,
        required_tier: 4,
    }
    .expanded(1.28)
}

/// Mission five opens the Verdant branch with a larger escort map. Relays are
/// optional for victory, but restoring them pays for the route and creates a
/// meaningful choice between infrastructure and protecting Sena's signal.
pub fn garden_below() -> MissionDef {
    let extraction = Vec2::new(1_120.0, 460.0);
    MissionDef {
        id: "garden-below",
        title: "THE GARDEN BELOW",
        briefing_story:
            "SENA QUILL: ROOTS ARE MOVING UNDER THE HULL. GET ME TO THE GARDEN ARRAY BEFORE THE CHOIR PRUNES IT.",
        victory_title: "GARDEN AWAKENED",
        victory_story:
            "LUMEN: THE GREEN SIGNAL IS A MEMORY WITH TEETH. I WILL TEACH IT YOUR NAME.",
        defeat_title: "SIGNAL PRUNED",
        defeat_story: "THE GARDEN CLOSES. VERDANT LIGHT DIES UNDER THE STATION.",
        relays: vec![
            Vec2::new(-760.0, -260.0),
            Vec2::new(20.0, 320.0),
            Vec2::new(820.0, -220.0),
        ],
        salvage_nodes: vec![
            // Keep this southwest cache just above the expanded root wall so
            // the Surveyor can path to it instead of spawning inside cover.
            Vec2::new(-1_040.0, -500.0),
            Vec2::new(-240.0, 100.0),
            Vec2::new(480.0, -40.0),
            Vec2::new(950.0, 400.0),
            Vec2::new(-1_040.0, 620.0),
            Vec2::new(1_320.0, -620.0),
        ],
        radio_lines: vec![
            RadioLine {
                speaker: "SENA QUILL",
                text: "THE GARDEN IS NOT PLANT LIFE. IT IS A MAP THAT LEARNED TO BREATHE.",
                trigger: DialogueTrigger::Time(2.0),
            },
            RadioLine {
                speaker: "IVO ROOK",
                text: "THREE RELAYS CAN PAY FOR THE ESCORT. DO NOT LET THE ROOTS CUT OUR RETURN LANE.",
                trigger: DialogueTrigger::Time(10.0),
            },
            // This warning is intentionally time-gated and authored before
            // the first relay condition. The ordered radio cursor can wait
            // on a later gated line, so the contest briefing must never
            // depend on the player staffing the node or restoring a relay.
            RadioLine {
                speaker: "SENA QUILL",
                text: "THE MIDDLE CACHE IS CONTESTED. SEND MARA FIRST, THEN LET ME WORK THE ROOTS.",
                trigger: DialogueTrigger::Time(18.0),
            },
            // Keep the build prompt in the same non-blocking prefix: it
            // teaches the player to turn the first cache load into a Warden
            // queue without waiting for either the relay or objective state.
            RadioLine {
                speaker: "IVO ROOK",
                text: "FABRICATOR HAS A SLOT. SPEND THE CACHE'S FIRST LOAD ON A WARDEN, THEN HOLD THE ROOTS.",
                trigger: DialogueTrigger::Time(24.0),
            },
            // Keep the Engineer's repair beat in the unconditional briefing
            // prefix. The player can hear the role assignment before any
            // relay gate, even when the eastern flank remains unexplored.
            RadioLine {
                speaker: "IVO ROOK",
                text: "THE REACTOR APRON IS BROKEN. HOLD MARA ON THE EASTERN FLANK WHILE I REPAIR THE CORE.",
                trigger: DialogueTrigger::Time(30.0),
            },
            RadioLine {
                speaker: "MARA VEY",
                text: "RIDGE AHEAD. TAKE THE HIGH GROUND, THEN WALK SENA THROUGH THE BLOOM.",
                trigger: DialogueTrigger::RelaysOnline(1),
            },
            RadioLine {
                speaker: "IVO ROOK",
                text: "SALVAGE RETURNED. I CAN KEEP THE FABRICATOR FED IF THE LINE HOLDS.",
                trigger: DialogueTrigger::SalvageDelivered(24),
            },
            RadioLine {
                speaker: "MARA VEY",
                text: "BELL MINE IN THE VINES. WARDEN FIRST, ENGINEER WIDE, SENA CENTER.",
                trigger: DialogueTrigger::UnitDestroyed(UnitKind::BellMine),
            },
            RadioLine {
                speaker: "LUMEN",
                text: "THE CHOIR IS PRUNING THE ARRAY. I HAVE OPENED A LIT PATH TO THE ROOT CHAMBER.",
                trigger: DialogueTrigger::EnemyRaid(1),
            },
            RadioLine {
                speaker: "SENA QUILL",
                text: "I CAN HEAR THE GARDEN ANSWERING. KEEP THE FORMATION CLOSE TO EXTRACTION.",
                trigger: DialogueTrigger::RelaysOnline(2),
            },
            RadioLine {
                speaker: "MARA VEY",
                text: "NO ONE RUNS AHEAD. WE CROSS THE ROOT CHAMBER TOGETHER.",
                trigger: DialogueTrigger::Time(42.0),
            },
            RadioLine {
                speaker: "SENA QUILL",
                text: "THE CACHE IS SECURE. THE GARDEN HAS A SECOND HEART—NOW WE CAN AFFORD TO REACH IT.",
                trigger: DialogueTrigger::ResourceObjectiveCompleted,
            },
        ],
        reactor_position: Some(Vec2::new(650.0, -60.0)),
        fabricator_position: Vec2::new(-1_120.0, -320.0),
        player_spawns: vec![
            PlayerSpawn::new(UnitKind::Warden, Vec2::new(-980.0, -260.0), 180.0, 175.0)
                .named("MARA VEY"),
            PlayerSpawn::new(UnitKind::Engineer, Vec2::new(-1_080.0, -350.0), 135.0, 150.0)
                .named("IVO ROOK"),
            PlayerSpawn::new(UnitKind::Surveyor, Vec2::new(-900.0, -350.0), 95.0, 210.0)
                .named("SENA QUILL")
                .escort(),
        ],
        enemy_spawns: vec![
            EnemySpawn::new(UnitKind::Needle, Vec2::new(-540.0, 300.0), 100.0, 130.0),
            EnemySpawn::new(UnitKind::BellMine, Vec2::new(-220.0, -120.0), 105.0, 80.0),
            // Keep the Needle outside the central vertical root wall so its
            // first order can actually reach the player formation.
            EnemySpawn::new(UnitKind::Needle, Vec2::new(140.0, 420.0), 100.0, 130.0),
            EnemySpawn::new(UnitKind::BellMine, Vec2::new(480.0, -340.0), 105.0, 80.0),
            EnemySpawn::new(UnitKind::Needle, Vec2::new(820.0, 220.0), 100.0, 130.0),
            EnemySpawn::new(UnitKind::Canticle, Vec2::new(760.0, -20.0), 420.0, 120.0),
        ],
        obstacles: vec![
            Aabb::from_center_size(Vec2::new(-420.0, 560.0), Vec2::new(1_700.0, 90.0)),
            Aabb::from_center_size(Vec2::new(-420.0, -590.0), Vec2::new(1_700.0, 90.0)),
            Aabb::from_center_size(Vec2::new(230.0, 100.0), Vec2::new(100.0, 760.0)),
            Aabb::from_center_size(Vec2::new(760.0, 280.0), Vec2::new(100.0, 460.0)),
        ],
        terrain_zones: vec![
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(-260.0, 230.0), Vec2::new(1_050.0, 360.0)),
                1,
                0.2,
            ),
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(-760.0, -260.0), Vec2::new(420.0, 360.0)),
                0,
                0.28,
            ),
            // The eastern Salvage route is an optional flank: a narrow
            // elevated perch around the third cache gives the Warden a
            // firing angle, but its lighter cover keeps the safe relay apron
            // from becoming the only correct answer.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(460.0, -30.0), Vec2::new(240.0, 170.0)),
                1,
                0.16,
            ),
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(780.0, -220.0), Vec2::new(460.0, 420.0)),
                0,
                0.3,
            ),
            // The Engineer's reactor repair is a deliberate late-game hold:
            // this compact apron makes the job readable without turning the
            // whole eastern root chamber into a safe zone.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(650.0, -60.0), Vec2::new(300.0, 220.0)),
                0,
                0.22,
            ),
            TerrainZone::new(
                Aabb::from_center_size(extraction, Vec2::new(420.0, 300.0)),
                0,
                0.26,
            ),
        ],
        lumen_console: Some(extraction),
        specialist_objective: None,
        engineer_repair_objective: Some(EngineerRepairObjective::new(
            Vec2::new(650.0, -60.0),
            105.0,
            9.0,
        )),
        terrain_control_objective: None,
        resource_objective: Some(ResourceObjective::secure_node(
            1,
            UnitKind::Surveyor,
            Some(UnitKind::Warden),
            120.0,
            250.0,
            260.0,
            8.0,
        )),
        victory: VictoryCondition::EscortToExtraction {
            point: extraction,
            radius: 160.0,
        },
        unlock_next: 6,
        reward_lumen: 120,
        unlock_decision: Some("verdant-cultivated"),
        environment_plate: TextureAsset::ReactorSectorGarden,
        required_tier: 5,
    }
    .expanded(1.12)
}

/// Mission six turns the terrain-control beat into a signal-warfare chapter.
/// The player must restore a three-node relay lattice while holding the
/// eastern sensor deck and securing a contested middle cache. Four clean
/// blockers create two readable routes around the blackout chambers: the
/// lower maintenance lane is safer for workers, while the upper signal deck
/// is faster but exposes the Warden to Choir fire.
pub fn choir_invisible() -> MissionDef {
    MissionDef {
        id: "choir-invisible",
        title: "CHOIR INVISIBLE",
        briefing_story:
            "SENA QUILL: CANTOR NINE HAS HIDDEN ITS RELAYS. GIVE ME COVER, THEN MAKE THE GRID CONFESS.",
        victory_title: "SIGNAL RELAYS SILENT",
        victory_story:
            "OLAN VOSS: THE CHOIR WAS NEVER INVISIBLE. IT WAS TEACHING US TO LOOK IN THE WRONG PLACE.",
        defeat_title: "GRID BLIND",
        defeat_story:
            "THE SENSOR LATTICE FOLDS. THE CHOIR OWNS EVERY DARK CORRIDOR.",
        relays: vec![
            Vec2::new(-820.0, 360.0),
            Vec2::new(-20.0, -360.0),
            Vec2::new(760.0, 300.0),
        ],
        salvage_nodes: vec![
            // The nearby pocket starts the worker loop behind the first
            // blackout wall; the middle cache is the authored contest beat.
            Vec2::new(-780.0, -150.0),
            Vec2::new(-40.0, 160.0),
            Vec2::new(700.0, -260.0),
            // Fourth and later nodes are Flux under the runtime's index
            // contract. They are deliberately off the direct relay route.
            Vec2::new(-1_180.0, 580.0),
            Vec2::new(1_180.0, 600.0),
            Vec2::new(1_220.0, -700.0),
        ],
        radio_lines: vec![
            RadioLine {
                speaker: "SENA QUILL",
                text: "CANTOR NINE IS BLINDING THE GRID. KEEP MY SCAN LANE OPEN AND I CAN EXPOSE ITS RELAYS.",
                trigger: DialogueTrigger::Time(2.0),
            },
            RadioLine {
                speaker: "MARA VEY",
                text: "THEN WE MAKE OUR OWN LIGHT. WARDEN TAKES THE DECK; ENGINEER AND SURVEYOR USE THE LOW LANE.",
                trigger: DialogueTrigger::Time(9.0),
            },
            RadioLine {
                speaker: "IVO ROOK",
                text: "THE MIDDLE CACHE IS HOT. KEEP SENA ON THE NODE WHILE MARA CLEARS THE BLACKOUT CHAMBER.",
                trigger: DialogueTrigger::Time(16.0),
            },
            RadioLine {
                speaker: "SENA QUILL",
                text: "FIRST RELAY MARKED. THE SIGNAL IS REAL—IT IS HIDING BEHIND THE NEXT WALL.",
                trigger: DialogueTrigger::RelaysOnline(1),
            },
            RadioLine {
                speaker: "MARA VEY",
                text: "THE DECK IS OURS. HOLD IT LONG ENOUGH FOR SENA TO THREAD THE LATTICE.",
                trigger: DialogueTrigger::RelaysOnline(2),
            },
            RadioLine {
                speaker: "IVO ROOK",
                text: "SALVAGE RETURNED. SPEND IT ON A WARDEN BEFORE THE EASTERN RELAY WAKES.",
                trigger: DialogueTrigger::SalvageDelivered(24),
            },
            RadioLine {
                speaker: "SENA QUILL",
                text: "THE CACHE IS SECURE. I CAN SEE CANTOR NINE'S APPROACH—IT IS SENDING A RAID THROUGH THE LOW LANE.",
                trigger: DialogueTrigger::ResourceObjectiveCompleted,
            },
            RadioLine {
                speaker: "OLAN VOSS",
                text: "BELL MINE IN THE SIGNAL FOG. DO NOT CHASE THE CONTACT; LET IT CROSS YOUR FIRING LINE.",
                trigger: DialogueTrigger::UnitDestroyed(UnitKind::BellMine),
            },
            RadioLine {
                speaker: "SENA QUILL",
                text: "SECOND RELAY IS CLEAR. THE LAST ONE IS ON THE EASTERN DECK—KEEP THE WARDEN UP THERE.",
                trigger: DialogueTrigger::RelaysOnline(2),
            },
            RadioLine {
                speaker: "OLAN VOSS",
                text: "CANTOR NINE IS SPEAKING THROUGH THE REACTOR. SILENCE THE VOICE, NOT THE LIGHT.",
                trigger: DialogueTrigger::EnemyRaid(1),
            },
        ],
        reactor_position: Some(Vec2::new(980.0, -40.0)),
        fabricator_position: Vec2::new(-1_180.0, -360.0),
        player_spawns: vec![
            PlayerSpawn::new(UnitKind::Warden, Vec2::new(-1_060.0, -260.0), 185.0, 175.0)
                .named("MARA VEY"),
            PlayerSpawn::new(UnitKind::Engineer, Vec2::new(-1_140.0, -340.0), 140.0, 150.0)
                .named("IVO ROOK"),
            PlayerSpawn::new(UnitKind::Surveyor, Vec2::new(-960.0, -360.0), 100.0, 215.0)
                .named("SENA QUILL"),
        ],
        enemy_spawns: vec![
            EnemySpawn::new(UnitKind::Needle, Vec2::new(-600.0, 420.0), 105.0, 130.0),
            EnemySpawn::new(UnitKind::BellMine, Vec2::new(120.0, -120.0), 105.0, 80.0),
            EnemySpawn::new(UnitKind::Needle, Vec2::new(500.0, 260.0), 105.0, 130.0),
            EnemySpawn::new(UnitKind::BellMine, Vec2::new(760.0, -320.0), 105.0, 80.0),
            EnemySpawn::new(UnitKind::Needle, Vec2::new(1_060.0, 320.0), 105.0, 130.0),
            EnemySpawn::new(UnitKind::Canticle, Vec2::new(1_000.0, -40.0), 450.0, 120.0),
        ],
        obstacles: vec![
            // Left blackout chamber: the opening is below the wall for the
            // worker lane, while the upper ridge remains the pressure route.
            Aabb::from_center_size(Vec2::new(-320.0, 40.0), Vec2::new(100.0, 760.0)),
            // Central sensor wall splits the middle cache from the eastern
            // relay and forces a choice between the high deck and low lane.
            Aabb::from_center_size(Vec2::new(380.0, 370.0), Vec2::new(100.0, 700.0)),
            // Low maintenance bulkhead leaves the southern Flux pocket as a
            // risky optional flank rather than a free early income source.
            Aabb::from_center_size(Vec2::new(760.0, -520.0), Vec2::new(800.0, 100.0)),
            // A top boundary plate keeps the northern Flux cache readable on
            // the minimap without sealing the route around the wall ends.
            Aabb::from_center_size(Vec2::new(0.0, 860.0), Vec2::new(2_600.0, 90.0)),
        ],
        terrain_zones: vec![
            // Pressure lane: the sensor deck resolves as high ground around
            // the eastern relay, and is the only terrain that completes the
            // Warden's terrain-control objective.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(760.0, 300.0), Vec2::new(320.0, 240.0)),
                1,
                0.12,
            ),
            // Safe worker lane near the Fabricator and opening Salvage.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(-780.0, -150.0), Vec2::new(360.0, 260.0)),
                0,
                0.26,
            ),
            // Contested cache apron: light cover buys the Surveyor time, but
            // the larger support radius means Mara must still clear contacts.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(-40.0, 160.0), Vec2::new(320.0, 220.0)),
                0,
                0.24,
            ),
            // The eastern reactor is an exposed late-game hold point. Its
            // low cover overlaps neither the sensor deck nor the bulkhead.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(980.0, -40.0), Vec2::new(300.0, 220.0)),
                0,
                0.18,
            ),
            // A small northern high-ground strip makes the Flux route a
            // defensible but optional scan-pulse detour.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(1_180.0, 600.0), Vec2::new(260.0, 180.0)),
                1,
                0.08,
            ),
        ],
        lumen_console: None,
        specialist_objective: None,
        engineer_repair_objective: None,
        terrain_control_objective: Some(TerrainControlObjective::high_ground_hold(
            Vec2::new(760.0, 300.0),
            110.0,
            UnitKind::Warden,
            8.0,
        )),
        resource_objective: Some(ResourceObjective::secure_node(
            1,
            UnitKind::Surveyor,
            Some(UnitKind::Warden),
            115.0,
            230.0,
            270.0,
            9.0,
        )),
        victory: VictoryCondition::RestoreRelaysAndDefeatBoss {
            boss_kind: UnitKind::Canticle,
        },
        unlock_next: 7,
        reward_lumen: 140,
        unlock_decision: Some("choir-invisible-cleared"),
        environment_plate: TextureAsset::ReactorSectorChoir,
        required_tier: 6,
    }
    .expanded(1.16)
}

/// Mission seven turns the campaign branch into a gate assault: the player
/// must keep a Surveyor on the Vesper cache, repair the auxiliary reactor with
/// the Engineer, and hold the eastern ridge while the three relays come online.
/// Two vertical gate walls split a safe maintenance lane from an exposed
/// northern flank, so the map rewards deliberate formation movement instead of
/// a single direct attack path.
pub fn vesper_gate() -> MissionDef {
    MissionDef {
        id: "vesper-gate",
        title: "THE VESPER GATE",
        briefing_story:
            "MARA VEY: THE SHUTTLE IS TRAPPED BEHIND A DEAD GATE. SECURE THE CACHE, REPAIR THE REACTOR, AND OPEN OUR WAY OUT.",
        victory_title: "VESPER CORRIDOR OPEN",
        victory_story:
            "SENA QUILL: THE GATE IS A MAP, NOT A WALL. I CAN TRACE A SAFE ROUTE THROUGH THE DARK.",
        defeat_title: "GATE SEALED",
        defeat_story:
            "IVO ROOK: THE AUXILIARY REACTOR DROPS OUT. VESPER IS CUT OFF FOR GOOD.",
        relays: vec![
            Vec2::new(-760.0, 420.0),
            Vec2::new(0.0, -360.0),
            Vec2::new(780.0, 420.0),
        ],
        salvage_nodes: vec![
            // Safe opening pocket and contested middle cache.
            Vec2::new(-980.0, -240.0),
            Vec2::new(-90.0, 180.0),
            Vec2::new(900.0, -220.0),
            // Optional Flux caches on the northern flank.
            Vec2::new(-1_460.0, 720.0),
            Vec2::new(1_450.0, 720.0),
            Vec2::new(40.0, 780.0),
        ],
        radio_lines: vec![
            RadioLine {
                speaker: "MARA VEY",
                text: "VESPER IS STILL BREATHING. KEEP THE SQUAD TOGETHER UNTIL I CAN SEE THE GATE CONTROLS.",
                trigger: DialogueTrigger::Time(2.0),
            },
            RadioLine {
                speaker: "SENA QUILL",
                text: "THE MIDDLE CACHE IS BROADCASTING A FALSE EXIT. LET ME MARK THE REAL ROUTE BEFORE THE CHOIR ARRIVES.",
                trigger: DialogueTrigger::Time(10.0),
            },
            RadioLine {
                speaker: "IVO ROOK",
                text: "THE AUXILIARY REACTOR IS COLD. ENGINEER ON THE APRON, WARDEN ON THE RIDGE, AND I CAN BRING IT BACK.",
                trigger: DialogueTrigger::Time(18.0),
            },
            RadioLine {
                speaker: "SENA QUILL",
                text: "CACHE MARKED. THE GATE'S SIGNAL IS REAL—IT IS HIDING BEHIND THE SECOND WALL.",
                trigger: DialogueTrigger::ResourceObjectiveCompleted,
            },
            RadioLine {
                speaker: "MARA VEY",
                text: "THE RIDGE IS OURS. HOLD THE EASTERN APPROACH WHILE IVO RESTARTS THE REACTOR.",
                trigger: DialogueTrigger::RelaysOnline(1),
            },
            RadioLine {
                speaker: "IVO ROOK",
                text: "REACTOR CORE IS RESPONDING. KEEP THE LOW LANE CLEAR AND DO NOT CHASE CONTACTS THROUGH THE FLANK.",
                trigger: DialogueTrigger::SalvageDelivered(30),
            },
            RadioLine {
                speaker: "OLAN VOSS",
                text: "THE CHOIR IS USING THE NORTHERN CACHES AS BAIT. LET THE TERRAIN DO SOME OF THE WORK.",
                trigger: DialogueTrigger::EnemyRaid(1),
            },
            RadioLine {
                speaker: "SENA QUILL",
                text: "SECOND RELAY IS OPEN. THE GATE CONTROL IS EAST—THE CANTICLE IS HIDING BEHIND ITS OWN LIGHT.",
                trigger: DialogueTrigger::RelaysOnline(2),
            },
            RadioLine {
                speaker: "MARA VEY",
                text: "BELL MINE IN THE MAINTENANCE LANE. HOLD FIRE UNTIL IT CROSSES THE RIDGE LINE.",
                trigger: DialogueTrigger::UnitDestroyed(UnitKind::BellMine),
            },
            RadioLine {
                speaker: "IVO ROOK",
                text: "VESPER GATE CONTROL IS LIVE. FINISH THE CANTICLE AND WE GET EVERYONE HOME.",
                trigger: DialogueTrigger::RelaysOnline(3),
            },
        ],
        reactor_position: Some(Vec2::new(1_080.0, 0.0)),
        fabricator_position: Vec2::new(-1_320.0, -500.0),
        player_spawns: vec![
            PlayerSpawn::new(UnitKind::Warden, Vec2::new(-1_180.0, -360.0), 190.0, 175.0)
                .named("MARA VEY"),
            PlayerSpawn::new(UnitKind::Engineer, Vec2::new(-1_280.0, -470.0), 145.0, 150.0)
                .named("IVO ROOK"),
            PlayerSpawn::new(UnitKind::Surveyor, Vec2::new(-980.0, -470.0), 105.0, 215.0)
                .named("SENA QUILL"),
        ],
        enemy_spawns: vec![
            EnemySpawn::new(UnitKind::Needle, Vec2::new(-520.0, 620.0), 110.0, 130.0),
            EnemySpawn::new(UnitKind::BellMine, Vec2::new(-80.0, -180.0), 105.0, 82.0),
            EnemySpawn::new(UnitKind::Needle, Vec2::new(620.0, 260.0), 110.0, 130.0),
            EnemySpawn::new(UnitKind::BellMine, Vec2::new(820.0, -180.0), 105.0, 82.0),
            EnemySpawn::new(UnitKind::Needle, Vec2::new(1_180.0, 480.0), 110.0, 130.0),
            EnemySpawn::new(UnitKind::Canticle, Vec2::new(1_080.0, 0.0), 480.0, 120.0),
        ],
        obstacles: vec![
            // The western gate wall leaves a low maintenance opening and an
            // exposed north route around its top edge.
            Aabb::from_center_size(Vec2::new(-540.0, 120.0), Vec2::new(100.0, 700.0)),
            // The eastern gate wall separates the repaired reactor from the
            // ridge, making the Warden's hold a real tactical commitment.
            Aabb::from_center_size(Vec2::new(420.0, -80.0), Vec2::new(100.0, 760.0)),
            // A short cross-gate blocks the direct center line while keeping
            // the southern relay route readable on the minimap.
            Aabb::from_center_size(Vec2::new(0.0, 360.0), Vec2::new(700.0, 100.0)),
            // Southern bulkhead protects the optional Flux pocket from free
            // early income and leaves the reactor approach exposed.
            Aabb::from_center_size(Vec2::new(700.0, -520.0), Vec2::new(900.0, 90.0)),
            Aabb::from_center_size(Vec2::new(0.0, 920.0), Vec2::new(3_000.0, 80.0)),
        ],
        terrain_zones: vec![
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(780.0, 420.0), Vec2::new(340.0, 240.0)),
                1,
                0.12,
            ),
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(-980.0, -240.0), Vec2::new(400.0, 260.0)),
                0,
                0.28,
            ),
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(-90.0, 180.0), Vec2::new(340.0, 240.0)),
                0,
                0.22,
            ),
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(1_080.0, 0.0), Vec2::new(320.0, 240.0)),
                0,
                0.16,
            ),
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(40.0, 780.0), Vec2::new(300.0, 160.0)),
                1,
                0.08,
            ),
        ],
        lumen_console: None,
        specialist_objective: None,
        engineer_repair_objective: Some(EngineerRepairObjective::new(
            Vec2::new(1_080.0, 0.0),
            120.0,
            7.0,
        )),
        terrain_control_objective: Some(TerrainControlObjective::high_ground_hold(
            Vec2::new(780.0, 420.0),
            110.0,
            UnitKind::Warden,
            10.0,
        )),
        resource_objective: Some(ResourceObjective::secure_node(
            1,
            UnitKind::Surveyor,
            Some(UnitKind::Warden),
            115.0,
            220.0,
            280.0,
            10.0,
        )),
        victory: VictoryCondition::RestoreRelaysAndDefeatBoss {
            boss_kind: UnitKind::Canticle,
        },
        unlock_next: 8,
        reward_lumen: 160,
        unlock_decision: Some("vesper-gate-open"),
        environment_plate: TextureAsset::ReactorSectorVesper,
        required_tier: 7,
    }
    .expanded(1.08)
}

/// Mission eight turns the campaign's gate route into an orbital-ring assault.
/// The player must keep the Warden on the eastern ridge, keep the Surveyor
/// alive on a Canticle-exposed dead-orbit cache, and bring the Engineer through
/// a mined coolant lane to restart the reactor. Twin bulkheads and a central
/// ring wall leave three readable routes instead of one direct boss rush.
pub fn hollow_orbit() -> MissionDef {
    MissionDef {
        id: "hollow-orbit",
        title: "THE HOLLOW ORBIT",
        briefing_story:
            "MARA VEY: VESPER'S ROUTE ENDS AT THE HOLLOW ORBIT. ANCHOR THE RIDGE, SECURE THE DEAD CACHE, AND WAKE THE COOLANT CORE.",
        victory_title: "ORBITAL LANTERN LIT",
        victory_story:
            "SENA QUILL: THE ORBIT IS NOT EMPTY. WE GAVE ITS LAST LIGHT A DIRECTION.",
        defeat_title: "ORBIT COLLAPSED",
        defeat_story:
            "IVO ROOK: THE COOLANT LOOP FAILS. THE RING TAKES THE LANTERN WITH IT.",
        relays: vec![
            Vec2::new(-940.0, 500.0),
            Vec2::new(0.0, -500.0),
            Vec2::new(940.0, 500.0),
        ],
        salvage_nodes: vec![
            // Opening pocket, central dead-orbit contest, and exposed eastern
            // cache define the worker route before the optional Flux arcs.
            Vec2::new(-1_080.0, -240.0),
            Vec2::new(-60.0, 160.0),
            Vec2::new(1_080.0, -180.0),
            // Optional northern/southern arcs reward a Surveyor detour after
            // the first relay, but do not pay for a blind opening rush.
            Vec2::new(-1_450.0, 740.0),
            Vec2::new(1_450.0, 740.0),
            Vec2::new(0.0, 790.0),
            Vec2::new(-1_450.0, -740.0),
        ],
        radio_lines: vec![
            // Keep every role's counterplay instruction in the unconditional
            // prefix so a stalled objective cannot hide the teaching beat.
            RadioLine {
                speaker: "MARA VEY",
                text: "THE HOLLOW RING IS SPLIT. WARDEN, ANCHOR THE EASTERN RIDGE—NEEDLES WANT YOU TO CHASE.",
                trigger: DialogueTrigger::Time(2.0),
            },
            RadioLine {
                speaker: "SENA QUILL",
                text: "THE DEAD-ORBIT CACHE IS UNDER CANTICLE FIRE. GIVE ME THE WARDEN'S FIRING LINE, NOT A RUN AHEAD.",
                trigger: DialogueTrigger::Time(10.0),
            },
            RadioLine {
                speaker: "IVO ROOK",
                text: "THE COOLANT CORE IS MINED. ENGINEER, WORK FROM COVER; PULL BELL MINES INTO MARA'S RANGE.",
                trigger: DialogueTrigger::Time(18.0),
            },
            RadioLine {
                speaker: "MARA VEY",
                text: "CLEAR THE MINE LANE, THEN SEND SENA TO THE CACHE. THREE JOBS, ONE RING.",
                trigger: DialogueTrigger::Time(26.0),
            },
            RadioLine {
                speaker: "SENA QUILL",
                text: "DEAD-ORBIT CACHE SECURE. THE CANTICLE'S FIRELINE IS BROKEN—MARKING THE NORTH ARC.",
                trigger: DialogueTrigger::ResourceObjectiveCompleted,
            },
            RadioLine {
                speaker: "MARA VEY",
                text: "RIDGE ANCHORED. KEEP ME ON THE HIGH BAND WHILE IVO CROSSES THE COOLANT BREAK.",
                trigger: DialogueTrigger::RelaysOnline(1),
            },
            RadioLine {
                speaker: "OLAN VOSS",
                text: "THE CHOIR IS PUSHING NEEDLES THROUGH THE WEST ARC. HOLD THE CHOKE; DO NOT FOLLOW THEIR BAIT.",
                trigger: DialogueTrigger::EnemyRaid(1),
            },
            RadioLine {
                speaker: "IVO ROOK",
                text: "SALVAGE RETURNED. THE COOLANT LOOP IS READY—KEEP THE ENGINEER COVERED FOR ONE MORE CYCLE.",
                trigger: DialogueTrigger::SalvageDelivered(36),
            },
            RadioLine {
                speaker: "SENA QUILL",
                text: "BELL MINE DOWN. THE EASTERN CACHE IS OPTIONAL; THE REACTOR ROUTE IS OUR WIN CONDITION.",
                trigger: DialogueTrigger::UnitDestroyed(UnitKind::BellMine),
            },
            RadioLine {
                speaker: "MARA VEY",
                text: "SECOND RELAY ONLINE. THE CANTICLE IS CUT OFF FROM THE RING—FINISH THE PUSH FROM COVER.",
                trigger: DialogueTrigger::RelaysOnline(2),
            },
            RadioLine {
                speaker: "OLAN VOSS",
                text: "CANTICLE FIRE IS FADING. KEEP THE FORMATION TIGHT AND LET THE RIDGE DO THE WORK.",
                trigger: DialogueTrigger::EnemyRaid(2),
            },
            RadioLine {
                speaker: "IVO ROOK",
                text: "THE HOLLOW ORBIT IS LIT. BREAK THE CANTICLE AND OPEN THE NEXT LANTERN ROUTE.",
                trigger: DialogueTrigger::RelaysOnline(3),
            },
        ],
        reactor_position: Some(Vec2::new(620.0, -420.0)),
        fabricator_position: Vec2::new(-1_320.0, -560.0),
        player_spawns: vec![
            PlayerSpawn::new(UnitKind::Warden, Vec2::new(-1_220.0, -420.0), 195.0, 175.0)
                .named("MARA VEY"),
            PlayerSpawn::new(UnitKind::Engineer, Vec2::new(-1_320.0, -520.0), 150.0, 150.0)
                .named("IVO ROOK"),
            PlayerSpawn::new(UnitKind::Surveyor, Vec2::new(-1_100.0, -520.0), 108.0, 215.0)
                .named("SENA QUILL"),
        ],
        enemy_spawns: vec![
            EnemySpawn::new(UnitKind::Needle, Vec2::new(-500.0, 620.0), 112.0, 132.0),
            EnemySpawn::new(UnitKind::BellMine, Vec2::new(-60.0, -280.0), 108.0, 82.0),
            EnemySpawn::new(UnitKind::Needle, Vec2::new(500.0, 620.0), 112.0, 132.0),
            EnemySpawn::new(UnitKind::BellMine, Vec2::new(620.0, -180.0), 108.0, 82.0),
            EnemySpawn::new(UnitKind::Needle, Vec2::new(1_200.0, 460.0), 112.0, 132.0),
            EnemySpawn::new(UnitKind::BellMine, Vec2::new(900.0, -320.0), 108.0, 82.0),
            EnemySpawn::new(UnitKind::Canticle, Vec2::new(980.0, -20.0), 500.0, 120.0),
        ],
        obstacles: vec![
            // Twin orbital bulkheads split the left and right service lanes;
            // the top and bottom ends remain open for deliberate flanks.
            Aabb::from_center_size(Vec2::new(-560.0, 40.0), Vec2::new(110.0, 900.0)),
            Aabb::from_center_size(Vec2::new(560.0, 40.0), Vec2::new(110.0, 900.0)),
            // Central ring plate creates a northern pressure route above the
            // dead cache, while the lower break protects the reactor approach.
            Aabb::from_center_size(Vec2::new(0.0, 420.0), Vec2::new(900.0, 90.0)),
            Aabb::from_center_size(Vec2::new(0.0, -360.0), Vec2::new(700.0, 90.0)),
            // The coolant bulkhead is a deliberate late-game choke rather
            // than a full seal: the Engineer can reach the reactor from east.
            Aabb::from_center_size(Vec2::new(1_020.0, -520.0), Vec2::new(700.0, 90.0)),
            Aabb::from_center_size(Vec2::new(0.0, 870.0), Vec2::new(3_000.0, 70.0)),
            Aabb::from_center_size(Vec2::new(0.0, -870.0), Vec2::new(3_000.0, 70.0)),
        ],
        terrain_zones: vec![
            // Only this elevated eastern band satisfies the Warden hold.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(940.0, 500.0), Vec2::new(360.0, 240.0)),
                1,
                0.12,
            ),
            // Covered opening pocket keeps the first Surveyor route readable.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(-1_080.0, -240.0), Vec2::new(420.0, 260.0)),
                0,
                0.28,
            ),
            // Dead-orbit cache is covered but exposed to the Canticle's
            // radius; Warden support must actually clear the contest.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(-60.0, 160.0), Vec2::new(360.0, 240.0)),
                0,
                0.24,
            ),
            // Coolant apron gives the Engineer a small repair pocket without
            // making the eastern ring safe from mines or artillery.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(620.0, -420.0), Vec2::new(320.0, 220.0)),
                0,
                0.22,
            ),
            // Optional eastern cache and north arc are light-cover perches.
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(1_080.0, -180.0), Vec2::new(300.0, 180.0)),
                1,
                0.1,
            ),
            TerrainZone::new(
                Aabb::from_center_size(Vec2::new(0.0, 790.0), Vec2::new(280.0, 180.0)),
                1,
                0.08,
            ),
        ],
        lumen_console: None,
        specialist_objective: None,
        engineer_repair_objective: Some(EngineerRepairObjective::new(
            Vec2::new(620.0, -420.0),
            110.0,
            8.0,
        )),
        terrain_control_objective: Some(TerrainControlObjective::high_ground_hold(
            Vec2::new(940.0, 500.0),
            110.0,
            UnitKind::Warden,
            12.0,
        )),
        resource_objective: Some(ResourceObjective::secure_node(
            1,
            UnitKind::Surveyor,
            Some(UnitKind::Warden),
            120.0,
            240.0,
            280.0,
            10.0,
        )),
        victory: VictoryCondition::RestoreRelaysAndDefeatBoss {
            boss_kind: UnitKind::Canticle,
        },
        unlock_next: 9,
        reward_lumen: 190,
        unlock_decision: Some("hollow-orbit-anchored"),
        environment_plate: TextureAsset::ReactorSectorHollow,
        required_tier: 8,
    }
    .expanded(1.18)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mission_state::{
        ResourceObjectiveAdvance, ResourceObjectiveState, SpecialistObjectiveKind,
        SpecialistObjectiveState, TerrainControlAdvance, TerrainControlPresence,
        TerrainControlState,
    };

    #[test]
    fn every_authored_mission_passes_the_playfield_contract() {
        for mission in all() {
            assert_eq!(
                mission.validate_layout(),
                Ok(()),
                "{} has invalid layout",
                mission.id
            );
        }
    }

    #[test]
    fn reclaim_offers_distinct_cover_and_high_ground_choices() {
        let mission = reclaim_the_reactor();
        assert!(mission.terrain_zones.len() >= 6);
        assert!(mission
            .terrain_zones
            .iter()
            .any(|zone| zone.elevation > 0 && zone.cover < 0.2));
        assert!(mission
            .terrain_zones
            .iter()
            .any(|zone| zone.elevation == 0 && zone.cover >= 0.2));
    }

    #[test]
    fn reclaim_authors_cover_for_the_opening_salvage_route() {
        let mission = reclaim_the_reactor();
        let opening_node = mission.salvage_nodes[0];
        assert!(mission.terrain_zones.iter().any(|zone| {
            zone.elevation == 0 && zone.cover >= 0.22 && zone.bounds.contains_point(opening_node)
        }));
    }

    #[test]
    fn reclaim_radio_lines_set_clear_role_jobs_early() {
        let mission = reclaim_the_reactor();
        let intro_cutoff = mission
            .radio_lines
            .iter()
            .position(|line| !matches!(line.trigger, DialogueTrigger::Time(time) if time <= 24.0))
            .unwrap_or(mission.radio_lines.len());

        let intro_slice = &mission.radio_lines[..intro_cutoff];
        let has_warden_verb = intro_slice.iter().any(|line| line.text.contains("WARDEN"));
        let has_engineer_verb = intro_slice
            .iter()
            .any(|line| line.text.contains("ENGINEER"));
        let has_surveyor_verb = intro_slice
            .iter()
            .any(|line| line.text.contains("SURVEYOR"));

        assert!(
            has_warden_verb,
            "reclaim should teach the Warden job before the relay phase"
        );
        assert!(
            has_engineer_verb,
            "reclaim should teach the Engineer job before the relay phase"
        );
        assert!(
            has_surveyor_verb,
            "reclaim should teach the Surveyor job before the relay phase"
        );
    }

    #[test]
    fn voice_in_conduit_radio_lines_teach_escort_job_distribution() {
        let mission = voice_in_conduit_twelve();
        let has_warden_verb = mission
            .radio_lines
            .iter()
            .any(|line| line.text.contains("WARDEN"));
        let has_engineer_verb = mission
            .radio_lines
            .iter()
            .any(|line| line.text.contains("ENGINEER"));
        let has_surveyor_verb = mission
            .radio_lines
            .iter()
            .any(|line| line.text.contains("SURVEYOR"));
        let has_escort_line = mission
            .radio_lines
            .iter()
            .any(|line| line.text.contains("ESCORT"));

        assert!(
            has_warden_verb,
            "escort mission should assign Warden behavior"
        );
        assert!(
            has_engineer_verb,
            "escort mission should assign Engineer repair behavior"
        );
        assert!(
            has_surveyor_verb,
            "escort mission should assign Surveyor behavior"
        );
        assert!(
            has_escort_line,
            "escort mission should explicitly use the escort role"
        );
    }

    #[test]
    fn terms_authors_warden_hold_in_the_vault_apron() {
        let mission = terms_of_salvage();
        let objective = mission
            .specialist_objective
            .expect("terms mission needs a Warden hold beat");
        assert_eq!(objective.kind, SpecialistObjectiveKind::WardenHold);
        assert_eq!(objective.kind.required_unit(), UnitKind::Warden);
        assert_eq!(objective.target, mission.relays[0]);
        assert!(mission
            .terrain_zones
            .iter()
            .any(|zone| zone.cover >= 0.2 && zone.bounds.contains_point(objective.target)));
    }

    #[test]
    fn terms_terrain_control_requires_the_resolved_high_ground_ridge() {
        let mission = terms_of_salvage();
        let objective = mission
            .terrain_control_objective
            .expect("terms mission needs a terrain-control beat");
        assert_eq!(objective.required_unit, UnitKind::Warden);
        assert_eq!(objective.target, mission.relays[0]);
        let (_, zone) = TerrainZone::resolve_at(objective.target, &mission.terrain_zones)
            .expect("terrain-control target resolves to a zone");
        assert!(zone.elevation >= objective.minimum_elevation);
        assert!(zone.cover >= objective.minimum_cover);

        let mut state = TerrainControlState::new();
        assert_eq!(
            state.advance(
                objective,
                TerrainControlPresence {
                    unit_kind: UnitKind::Warden,
                    unit_position: objective.target,
                    terrain_elevation: 0,
                    terrain_cover: 0.3,
                    enemy_present: false,
                },
                2.0,
            ),
            TerrainControlAdvance::WrongTerrain
        );
        assert_eq!(state.progress_seconds, 0.0);

        assert_eq!(
            state.advance(
                objective,
                TerrainControlPresence {
                    unit_kind: UnitKind::Warden,
                    unit_position: objective.target,
                    terrain_elevation: 1,
                    terrain_cover: zone.cover,
                    enemy_present: false,
                },
                2.0,
            ),
            TerrainControlAdvance::Progressed
        );
        assert_eq!(state.progress_seconds, 2.0);
        assert_eq!(
            state.advance(
                objective,
                TerrainControlPresence {
                    unit_kind: UnitKind::Warden,
                    unit_position: objective.target,
                    terrain_elevation: zone.elevation,
                    terrain_cover: zone.cover,
                    enemy_present: true,
                },
                1.0,
            ),
            TerrainControlAdvance::Contested
        );
        assert_eq!(state.progress_seconds, 2.0);
        assert_eq!(state.contested_seconds, 1.0);
        assert_eq!(
            state.advance(
                objective,
                TerrainControlPresence {
                    unit_kind: UnitKind::Warden,
                    unit_position: objective.target,
                    terrain_elevation: zone.elevation,
                    terrain_cover: zone.cover,
                    enemy_present: false,
                },
                4.0,
            ),
            TerrainControlAdvance::Completed
        );
        assert!(state.completed);
        assert_eq!(state.fraction(objective), 1.0);
    }

    #[test]
    fn terrain_control_validation_rejects_missing_high_ground() {
        let mut mission = terms_of_salvage();
        mission.terrain_zones.retain(|zone| zone.elevation <= 0);
        assert_eq!(
            mission.validate_layout(),
            Err("terrain control objective target needs matching terrain")
        );
    }

    #[test]
    fn garden_authors_engineer_repair_in_the_reactor_apron() {
        let mission = garden_below();
        let objective = mission
            .engineer_repair_objective
            .expect("garden mission needs an Engineer repair beat");
        let reactor = mission.reactor_position.expect("garden reactor");
        assert_eq!(objective.required_unit(), UnitKind::Engineer);
        assert_eq!(objective.target, reactor);
        assert!(mission.terrain_zones.iter().any(|zone| {
            let size = zone.bounds.size();
            zone.cover >= 0.2
                && size.x <= 360.0
                && size.y <= 260.0
                && zone.bounds.contains_point(objective.target)
        }));
    }

    #[test]
    fn garden_authors_a_contestable_middle_resource_objective() {
        let mission = garden_below();
        let objective = mission
            .resource_objective
            .expect("garden needs a worker resource beat");
        assert_eq!(objective.node_index, 1);
        assert_eq!(objective.worker_kind, UnitKind::Surveyor);
        assert_eq!(objective.support_kind, Some(UnitKind::Warden));
        assert!(objective.contest_radius > objective.worker_radius);
        let target = mission.salvage_nodes[objective.node_index];
        assert!(mission
            .terrain_zones
            .iter()
            .any(|zone| zone.elevation > 0 && zone.bounds.contains_point(target)));

        let mut state = ResourceObjectiveState::new();
        assert_eq!(
            state.advance(objective, true, false, true, 1.0),
            ResourceObjectiveAdvance::Contested
        );
        assert_eq!(state.progress_seconds, 0.0);
        assert_eq!(state.contested_seconds, 1.0);
        assert_eq!(
            state.advance(objective, true, true, true, 3.0),
            ResourceObjectiveAdvance::Progressed
        );
        assert_eq!(state.progress_seconds, 3.0);
        assert_eq!(state.contested_seconds, 1.0);
        assert_eq!(
            state.advance(objective, true, true, false, 5.0),
            ResourceObjectiveAdvance::Completed
        );
        assert!(state.completed);
        assert_eq!(state.fraction(objective), 1.0);
    }

    #[test]
    fn garden_eastern_cache_offers_a_deterministic_flank_perch() {
        let mission = garden_below();
        let flank_cache = mission.salvage_nodes[2];
        let perch = mission
            .terrain_zones
            .iter()
            .find(|zone| {
                zone.elevation > 0
                    && (0.1..0.2).contains(&zone.cover)
                    && zone.bounds.contains_point(flank_cache)
            })
            .expect("garden authors a light-cover eastern flank perch");

        // The optional flank has a better firing angle than open ground, but
        // does not overlap the eastern hard-cover pocket that protects the
        // reactor. That keeps both routes tactically legible after expansion.
        assert!(perch.cover < 0.2);
        assert!(!mission.terrain_zones.iter().any(|zone| {
            zone.elevation == 0 && zone.cover >= 0.3 && zone.bounds.contains_point(flank_cache)
        }));
        assert!(mission
            .obstacles
            .iter()
            .all(|obstacle| !obstacle.contains_point(flank_cache)));

        let objective = mission
            .resource_objective
            .expect("garden keeps a finite middle-cache pressure beat");
        assert!(objective.contest_radius > objective.support_radius);
        assert!(objective.support_radius > objective.worker_radius);
        assert_eq!(objective.required_seconds, 8.0);

        // Replaying the same worker/support/contest sequence must produce the
        // same state, including the one-second contested window.
        let sequence = [
            (true, false, true, 1.0),
            (true, true, true, 3.0),
            (true, true, false, 5.0),
        ];
        let mut first = ResourceObjectiveState::new();
        let mut second = ResourceObjectiveState::new();
        for (worker, support, enemy, dt) in sequence {
            assert_eq!(
                first.advance(objective, worker, support, enemy, dt),
                second.advance(objective, worker, support, enemy, dt)
            );
        }
        assert_eq!(first, second);
        assert!(first.completed);
        assert_eq!(first.contested_seconds, 1.0);
    }

    #[test]
    fn garden_contest_callout_is_ambient_before_gated_radio_lines() {
        let mission = garden_below();
        let contest_index = mission
            .radio_lines
            .iter()
            .position(|line| line.text.contains("MIDDLE CACHE IS CONTESTED"))
            .expect("garden authors a contest warning");
        let completion_index = mission
            .radio_lines
            .iter()
            .position(|line| matches!(line.trigger, DialogueTrigger::ResourceObjectiveCompleted))
            .expect("garden authors a resource completion handoff");

        assert!(contest_index < completion_index);
        assert!(matches!(
            mission.radio_lines[contest_index].trigger,
            DialogueTrigger::Time(time) if (time - 18.0).abs() < f32::EPSILON
        ));
        // The two opening lines and this warning are all time-gated. That
        // makes the contest telemetry reachable even when no worker ever
        // reaches the node; later objective/relay gates cannot deadlock it.
        assert!(mission.radio_lines[..=contest_index]
            .iter()
            .all(|line| matches!(line.trigger, DialogueTrigger::Time(_))));
    }

    #[test]
    fn choir_invisible_is_the_next_unlock_after_garden() {
        let garden = garden_below();
        let mission = choir_invisible();

        assert_eq!(mission.required_tier, garden.unlock_next);
        assert_eq!(mission.required_tier, 6);
        assert_eq!(mission.unlock_next, 7);
        assert_eq!(mission.reward_lumen, 140);
        assert!(all().iter().any(|candidate| candidate.id == mission.id));
    }

    #[test]
    fn choir_invisible_resolves_a_pressure_deck_and_safe_cache() {
        let mission = choir_invisible();
        let terrain_objective = mission
            .terrain_control_objective
            .expect("signal warfare needs a Warden deck hold");
        let resource_objective = mission
            .resource_objective
            .expect("signal warfare needs a contested cache");

        assert_eq!(mission.relays.len(), 3);
        assert_eq!(mission.salvage_nodes.len(), 6);
        assert_eq!(terrain_objective.required_unit, UnitKind::Warden);
        assert_eq!(terrain_objective.target, mission.relays[2]);
        let (_, deck) = TerrainZone::resolve_at(terrain_objective.target, &mission.terrain_zones)
            .expect("eastern relay resolves to authored deck terrain");
        assert!(deck.elevation >= terrain_objective.minimum_elevation);
        assert!(deck.cover >= terrain_objective.minimum_cover);

        assert_eq!(resource_objective.node_index, 1);
        assert_eq!(resource_objective.worker_kind, UnitKind::Surveyor);
        assert_eq!(resource_objective.support_kind, Some(UnitKind::Warden));
        assert!(resource_objective.contest_radius > resource_objective.support_radius);
        assert!(resource_objective.support_radius > resource_objective.worker_radius);
        let cache = mission.salvage_nodes[resource_objective.node_index];
        let (_, cache_zone) = TerrainZone::resolve_at(cache, &mission.terrain_zones)
            .expect("middle cache resolves to a covered worker apron");
        assert_eq!(cache_zone.elevation, 0);
        assert!(cache_zone.cover >= 0.24);
        assert_ne!(cache, terrain_objective.target);
        assert!(mission
            .obstacles
            .iter()
            .all(|obstacle| !obstacle.contains_point(cache)
                && !obstacle.contains_point(terrain_objective.target)));
    }

    #[test]
    fn choir_invisible_keeps_signal_warfare_advice_before_relay_gates() {
        let mission = choir_invisible();
        let contest_index = mission
            .radio_lines
            .iter()
            .position(|line| line.text.contains("MIDDLE CACHE IS HOT"))
            .expect("signal map authors a contested cache warning");
        let first_gated_index = mission
            .radio_lines
            .iter()
            .position(|line| !matches!(line.trigger, DialogueTrigger::Time(_)))
            .expect("signal map has a relay-gated handoff");

        assert!(contest_index < first_gated_index);
        assert!(mission.radio_lines[..=contest_index]
            .iter()
            .all(|line| matches!(line.trigger, DialogueTrigger::Time(_))));
        assert!(mission
            .radio_lines
            .iter()
            .any(|line| matches!(line.trigger, DialogueTrigger::EnemyRaid(1))));
    }

    #[test]
    fn vesper_gate_is_the_next_branch_after_choir_invisible() {
        let previous = choir_invisible();
        let mission = vesper_gate();

        assert_eq!(mission.required_tier, previous.unlock_next);
        assert_eq!(mission.required_tier, 7);
        assert_eq!(mission.unlock_next, 8);
        assert_eq!(mission.reward_lumen, 160);
        assert_eq!(mission.unlock_decision, Some("vesper-gate-open"));
        assert!(all().iter().any(|candidate| candidate.id == mission.id));
    }

    #[test]
    fn vesper_gate_combines_three_role_contracts_without_collapsed_routes() {
        let mission = vesper_gate();
        let reactor = mission
            .reactor_position
            .expect("vesper needs an auxiliary reactor");
        let repair = mission
            .engineer_repair_objective
            .expect("vesper needs an Engineer repair beat");
        let terrain = mission
            .terrain_control_objective
            .expect("vesper needs a Warden ridge hold");
        let resource = mission
            .resource_objective
            .expect("vesper needs a Surveyor cache beat");

        assert_eq!(mission.relays.len(), 3);
        assert_eq!(mission.salvage_nodes.len(), 6);
        assert_eq!(repair.required_unit(), UnitKind::Engineer);
        assert_eq!(repair.target, reactor);
        assert_eq!(terrain.required_unit, UnitKind::Warden);
        assert_eq!(terrain.target, mission.relays[2]);
        assert_eq!(resource.worker_kind, UnitKind::Surveyor);
        assert_eq!(resource.support_kind, Some(UnitKind::Warden));
        assert!(resource.contest_radius > resource.support_radius);

        let (_, ridge) = TerrainZone::resolve_at(terrain.target, &mission.terrain_zones)
            .expect("eastern relay resolves to the Vesper ridge");
        assert!(ridge.elevation >= terrain.minimum_elevation);
        let cache = mission.salvage_nodes[resource.node_index];
        let (_, cache_zone) = TerrainZone::resolve_at(cache, &mission.terrain_zones)
            .expect("middle cache resolves to a worker lane");
        assert_eq!(cache_zone.elevation, 0);
        assert!(cache_zone.cover >= 0.2);
        assert!(mission
            .obstacles
            .iter()
            .all(|obstacle| !obstacle.contains_point(cache)
                && !obstacle.contains_point(terrain.target)
                && !obstacle.contains_point(reactor)));

        let first_gated = mission
            .radio_lines
            .iter()
            .position(|line| !matches!(line.trigger, DialogueTrigger::Time(_)))
            .expect("vesper needs a later gated handoff");
        assert!(mission.radio_lines[..first_gated]
            .iter()
            .any(|line| line.text.contains("CACHE IS BROADCASTING")));
        assert!(mission
            .radio_lines
            .iter()
            .any(|line| { matches!(line.trigger, DialogueTrigger::ResourceObjectiveCompleted) }));
    }

    #[test]
    fn hollow_orbit_is_the_next_unlock_after_vesper_gate() {
        let previous = vesper_gate();
        let mission = hollow_orbit();

        assert_eq!(mission.required_tier, previous.unlock_next);
        assert_eq!(mission.required_tier, 8);
        assert_eq!(mission.unlock_next, 9);
        assert_eq!(mission.reward_lumen, 190);
        assert_eq!(mission.unlock_decision, Some("hollow-orbit-anchored"));
        assert_eq!(all().last().map(|candidate| candidate.id), Some(mission.id));
    }

    #[test]
    fn hollow_orbit_gives_each_lantern_specialist_a_readable_counterplay_job() {
        let mission = hollow_orbit();
        let reactor = mission
            .reactor_position
            .expect("hollow orbit needs a coolant reactor");
        let repair = mission
            .engineer_repair_objective
            .expect("hollow orbit needs an Engineer repair beat");
        let terrain = mission
            .terrain_control_objective
            .expect("hollow orbit needs a Warden ridge hold");
        let resource = mission
            .resource_objective
            .expect("hollow orbit needs a Surveyor cache beat");

        assert_eq!(mission.relays.len(), 3);
        assert_eq!(mission.salvage_nodes.len(), 7);
        assert_eq!(repair.required_unit(), UnitKind::Engineer);
        assert_eq!(repair.target, reactor);
        assert_eq!(terrain.required_unit, UnitKind::Warden);
        assert_eq!(terrain.target, mission.relays[2]);
        assert_eq!(resource.node_index, 1);
        assert_eq!(resource.worker_kind, UnitKind::Surveyor);
        assert_eq!(resource.support_kind, Some(UnitKind::Warden));
        assert!(resource.contest_radius > resource.support_radius);
        assert!(resource.support_radius > resource.worker_radius);

        let (_, ridge) = TerrainZone::resolve_at(terrain.target, &mission.terrain_zones)
            .expect("eastern relay resolves to the orbital ridge");
        assert!(ridge.elevation >= terrain.minimum_elevation);
        let cache = mission.salvage_nodes[resource.node_index];
        let (_, cache_zone) = TerrainZone::resolve_at(cache, &mission.terrain_zones)
            .expect("dead-orbit cache resolves to authored cover");
        assert_eq!(cache_zone.elevation, 0);
        assert!(cache_zone.cover >= 0.2);
        assert!(mission
            .obstacles
            .iter()
            .all(|obstacle| !obstacle.contains_point(cache)
                && !obstacle.contains_point(terrain.target)
                && !obstacle.contains_point(reactor)));

        let first_gated = mission
            .radio_lines
            .iter()
            .position(|line| !matches!(line.trigger, DialogueTrigger::Time(_)))
            .expect("hollow orbit needs a later gated handoff");
        let opening = &mission.radio_lines[..first_gated];
        assert!(opening
            .iter()
            .all(|line| matches!(line.trigger, DialogueTrigger::Time(_))));
        assert!(opening
            .iter()
            .any(|line| line.text.contains("WARDEN") && line.text.contains("NEEDLES")));
        assert!(opening
            .iter()
            .any(|line| line.text.contains("ENGINEER") && line.text.contains("BELL MINES")));
        assert!(opening
            .iter()
            .any(|line| line.text.contains("SENA") && line.text.contains("CACHE")));
        assert!(mission
            .radio_lines
            .iter()
            .any(|line| matches!(line.trigger, DialogueTrigger::ResourceObjectiveCompleted)));
        assert!(mission.radio_lines.iter().any(|line| matches!(
            line.trigger,
            DialogueTrigger::UnitDestroyed(UnitKind::BellMine)
        )));
    }

    #[test]
    fn garden_build_prompt_keeps_resource_advice_before_optional_gates() {
        let mission = garden_below();
        let prompt_index = mission
            .radio_lines
            .iter()
            .position(|line| {
                line.text.contains("FABRICATOR HAS A SLOT")
                    && line.text.contains("CACHE'S FIRST LOAD")
            })
            .expect("garden authors a build/resource prompt");
        let first_gated_index = mission
            .radio_lines
            .iter()
            .position(|line| !matches!(line.trigger, DialogueTrigger::Time(_)))
            .expect("garden has a later optional gate");

        assert!(prompt_index < first_gated_index);
        assert!(matches!(
            mission.radio_lines[prompt_index].trigger,
            DialogueTrigger::Time(time) if (time - 24.0).abs() < f32::EPSILON
        ));
        assert!(mission.radio_lines[..=prompt_index]
            .iter()
            .all(|line| matches!(line.trigger, DialogueTrigger::Time(_))));
    }

    #[test]
    fn garden_repair_callout_is_deterministic_before_the_first_relay_gate() {
        let mission = garden_below();
        let repair_index = mission
            .radio_lines
            .iter()
            .position(|line| line.text.contains("REACTOR APRON IS BROKEN"))
            .expect("garden authors an Engineer repair callout");
        let first_gated_index = mission
            .radio_lines
            .iter()
            .position(|line| !matches!(line.trigger, DialogueTrigger::Time(_)))
            .expect("garden has a later relay gate");

        assert!(repair_index < first_gated_index);
        assert!(matches!(
            mission.radio_lines[repair_index].trigger,
            DialogueTrigger::Time(time) if (time - 30.0).abs() < f32::EPSILON
        ));
        assert_eq!(mission.radio_lines[repair_index].speaker, "IVO ROOK");
    }

    #[test]
    fn late_maps_preserve_flank_blockers_after_expansion() {
        let terms = terms_of_salvage();
        let garden = garden_below();
        assert!(terms.obstacles.len() >= 2);
        assert!(garden.obstacles.len() >= 4);
        assert!(terms
            .obstacles
            .iter()
            .all(|obstacle| obstacle.size().x > 0.0 && obstacle.size().y > 0.0));
        assert!(garden
            .obstacles
            .iter()
            .all(|obstacle| obstacle.size().x > 0.0 && obstacle.size().y > 0.0));
    }

    #[test]
    fn resource_contract_rejects_collapsed_worker_routes() {
        let mut mission = reclaim_the_reactor();
        mission.salvage_nodes[1] =
            mission.salvage_nodes[0] + Vec2::new(MIN_RESOURCE_NODE_SEPARATION - 1.0, 0.0);
        assert_eq!(
            mission.validate_layout(),
            Err("resource nodes are too close for distinct worker routes")
        );
    }

    #[test]
    fn escort_map_protects_its_contested_middle_resource() {
        let mission = voice_in_conduit_twelve();
        let middle_resource = mission.salvage_nodes[1];
        assert!(mission.terrain_zones.iter().any(|zone| {
            zone.elevation == 0 && zone.cover >= 0.24 && zone.bounds.contains_point(middle_resource)
        }));
    }

    #[test]
    fn vault_map_protects_only_the_safe_start_resource_pocket() {
        let mission = terms_of_salvage();
        let start_resource = mission.salvage_nodes[0];
        assert!(mission.terrain_zones.iter().any(|zone| {
            zone.elevation == 0 && zone.cover >= 0.24 && zone.bounds.contains_point(start_resource)
        }));
        assert!(!mission.terrain_zones.iter().any(|zone| {
            zone.cover >= 0.24 && zone.bounds.contains_point(mission.salvage_nodes[2])
        }));
    }

    #[test]
    fn vault_reactor_has_a_focused_defensive_apron() {
        let mission = terms_of_salvage();
        let reactor = mission.reactor_position.expect("vault needs a reactor");
        assert!(mission.terrain_zones.iter().any(|zone| {
            let size = zone.bounds.size();
            zone.cover >= 0.2
                && size.x <= 420.0
                && size.y <= 330.0
                && zone.bounds.contains_point(reactor)
        }));
    }

    #[test]
    fn voice_authors_a_surveyor_scan_inside_the_extraction_pocket() {
        let mission = voice_in_conduit_twelve();
        let objective = mission
            .specialist_objective
            .expect("voice mission needs a specialist scan beat");
        assert_eq!(objective.kind, SpecialistObjectiveKind::SurveyorScan);
        assert_eq!(objective.kind.required_unit(), UnitKind::Surveyor);
        assert_eq!(
            objective.target,
            mission.lumen_console.expect("signal console")
        );
        assert!(mission
            .terrain_zones
            .iter()
            .any(|zone| zone.bounds.contains_point(objective.target) && zone.cover >= 0.24));
        assert!(mission
            .player_spawns
            .iter()
            .any(|spawn| spawn.kind == objective.kind.required_unit()));
    }

    #[test]
    fn specialist_scan_progress_is_role_gated_and_deterministic() {
        let mission = voice_in_conduit_twelve();
        let objective = mission.specialist_objective.expect("scan objective");
        let mut state = SpecialistObjectiveState::new();

        assert!(!state.advance(objective, UnitKind::Engineer, objective.target, 10.0));
        assert_eq!(state.progress_seconds, 0.0);

        assert!(!state.advance(
            objective,
            UnitKind::Surveyor,
            objective.target + Vec2::new(objective.radius + 1.0, 0.0),
            2.0
        ));
        assert_eq!(state.progress_seconds, 0.0);

        assert!(!state.advance(objective, UnitKind::Surveyor, objective.target, 2.5));
        assert_eq!(state.progress_seconds, 2.5);
        assert!(state.advance(objective, UnitKind::Surveyor, objective.target, 3.5));
        assert_eq!(state.progress_seconds, objective.required_seconds);
        assert_eq!(state.fraction(objective), 1.0);

        // Completion is sticky, so a later movement event cannot undo the
        // campaign beat or make replay outcomes frame-rate dependent.
        assert!(state.advance(objective, UnitKind::Engineer, Vec2::ZERO, f32::NAN));
    }

    #[test]
    fn warden_hold_progress_is_role_gated_and_deterministic() {
        let mission = terms_of_salvage();
        let objective = mission.specialist_objective.expect("hold objective");
        let mut state = SpecialistObjectiveState::new();

        assert!(!state.advance(objective, UnitKind::Engineer, objective.target, 99.0));
        assert_eq!(state.progress_seconds, 0.0);
        assert!(!state.advance(objective, UnitKind::Warden, objective.target, 3.0));
        assert!(!state.advance(
            objective,
            UnitKind::Warden,
            objective.target + Vec2::new(objective.radius + 1.0, 0.0),
            3.0
        ));
        assert_eq!(state.progress_seconds, 3.0);
        assert!(state.advance(objective, UnitKind::Warden, objective.target, 4.0));
        assert_eq!(state.progress_seconds, objective.required_seconds);
    }

    #[test]
    fn engineer_repair_progress_is_role_gated_and_deterministic() {
        let mission = garden_below();
        let objective = mission.engineer_repair_objective.expect("repair objective");
        let mut state = SpecialistObjectiveState::new();

        assert!(!state.advance_engineer_repair(
            objective,
            UnitKind::Warden,
            objective.target,
            99.0
        ));
        assert_eq!(state.progress_seconds, 0.0);
        assert!(!state.advance_engineer_repair(
            objective,
            UnitKind::Engineer,
            objective.target,
            4.0
        ));
        assert!(!state.advance_engineer_repair(
            objective,
            UnitKind::Engineer,
            objective.target + Vec2::new(objective.radius + 1.0, 0.0),
            4.0
        ));
        assert_eq!(state.progress_seconds, 4.0);
        assert!(state.advance_engineer_repair(
            objective,
            UnitKind::Engineer,
            objective.target,
            5.0
        ));
        assert_eq!(state.progress_seconds, objective.required_seconds);
        assert_eq!(state.engineer_repair_fraction(objective), 1.0);
    }

    #[test]
    fn specialist_objective_contract_rejects_missing_role_and_bad_duration() {
        let mut mission = voice_in_conduit_twelve();
        mission
            .player_spawns
            .retain(|spawn| spawn.kind != UnitKind::Surveyor);
        assert_eq!(
            mission.validate_layout(),
            Err("specialist objective requires its unit role")
        );

        let mut objective = mission
            .specialist_objective
            .expect("scan objective remains authored");
        objective.required_seconds = 0.0;
        assert_eq!(
            objective.validate(),
            Err("specialist objective duration must be finite and positive")
        );

        let mut terrainless = garden_below();
        terrainless.terrain_zones.clear();
        assert_eq!(
            terrainless.validate_layout(),
            Err("engineer repair objective target needs authored terrain")
        );

        let mut objective = terrainless
            .engineer_repair_objective
            .expect("repair objective remains authored");
        objective.required_seconds = 0.0;
        assert_eq!(
            objective.validate(),
            Err("specialist objective duration must be finite and positive")
        );
    }
}
