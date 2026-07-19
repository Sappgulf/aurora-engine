//! Data-driven mission definitions for the Last Light campaign.
//!
//! Keeping map layout, spawns, and victory conditions as plain data (rather
//! than hardcoded in `main.rs`) is what lets a second mission reuse the same
//! simulation/rendering code instead of forking it.

use aurora_engine::Aabb;
use glam::Vec2;

use crate::units::UnitKind;

#[derive(Debug, Clone, Copy)]
pub struct PlayerSpawn {
    pub kind: UnitKind,
    pub position: Vec2,
    pub health: f32,
    pub speed: f32,
    pub escort: bool,
}

impl PlayerSpawn {
    pub fn new(kind: UnitKind, position: Vec2, health: f32, speed: f32) -> Self {
        Self {
            kind,
            position,
            health,
            speed,
            escort: false,
        }
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
    /// Position of the "wake Lumen" interaction, if this mission has one.
    pub lumen_console: Option<Vec2>,
    pub victory: VictoryCondition,
    pub unlock_next: u32,
    pub reward_lumen: u64,
    /// Campaign decision recorded automatically on victory (independent of
    /// any mid-mission choice like `lumen_console`).
    pub unlock_decision: Option<&'static str>,
    /// Minimum `CampaignProgress::unlocked_mission` tier required to select
    /// this mission from the mission-select screen.
    pub required_tier: u32,
}

/// All missions in campaign order, for the mission-select screen.
pub fn all() -> Vec<MissionDef> {
    vec![
        reclaim_the_reactor(),
        voice_in_conduit_twelve(),
        terms_of_salvage(),
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
        ],
        radio_lines: vec![
            RadioLine {
                speaker: "MARA VEY",
                text: "LANTERN TEAM, STAY TOGETHER. THE CHOIR HEARS ISOLATION.",
                trigger: DialogueTrigger::Time(2.0),
            },
            RadioLine {
                speaker: "IVO RENN",
                text: "SURVEYOR CAN SIP SALVAGE BLOOMS. KEEP IT CLOSE, KEEP IT FUNDED.",
                trigger: DialogueTrigger::Time(9.0),
            },
            RadioLine {
                speaker: "SENA QUILL",
                text: "FIRST RELAY IS SINGING BACK. THE DARK JUST GOT SMALLER.",
                trigger: DialogueTrigger::RelaysOnline(1),
            },
            RadioLine {
                speaker: "LUMEN",
                text: "THREE LIGHTS. ONE VOICE. COME FIND ME BENEATH THE REACTOR.",
                trigger: DialogueTrigger::RelaysOnline(3),
            },
        ],
        reactor_position: Some(Vec2::new(520.0, -40.0)),
        fabricator_position: Vec2::new(-1_020.0, -120.0),
        player_spawns: vec![
            PlayerSpawn::new(UnitKind::Warden, Vec2::new(-880.0, -290.0), 155.0, 175.0),
            PlayerSpawn::new(UnitKind::Engineer, Vec2::new(-790.0, -350.0), 115.0, 150.0),
            PlayerSpawn::new(UnitKind::Surveyor, Vec2::new(-900.0, -410.0), 90.0, 215.0),
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
        lumen_console: None,
        victory: VictoryCondition::RestoreRelaysAndDefeatBoss {
            boss_kind: UnitKind::Canticle,
        },
        unlock_next: 3,
        reward_lumen: 80,
        unlock_decision: Some("lumen-contact-established"),
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
        ],
        reactor_position: None,
        fabricator_position: Vec2::new(-1_100.0, 380.0),
        player_spawns: vec![
            PlayerSpawn::new(UnitKind::Warden, Vec2::new(-980.0, 300.0), 175.0, 175.0),
            PlayerSpawn::new(UnitKind::Engineer, Vec2::new(-1_040.0, 250.0), 130.0, 150.0),
            PlayerSpawn::new(UnitKind::Surveyor, Vec2::new(-900.0, 340.0), 90.0, 210.0).escort(),
        ],
        enemy_spawns: vec![
            EnemySpawn::new(UnitKind::Needle, Vec2::new(-400.0, 260.0), 90.0, 125.0),
            EnemySpawn::new(UnitKind::Needle, Vec2::new(-100.0, -60.0), 90.0, 125.0),
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
        lumen_console: Some(extraction),
        victory: VictoryCondition::EscortToExtraction {
            point: extraction,
            radius: 140.0,
        },
        unlock_next: 4,
        reward_lumen: 90,
        unlock_decision: None,
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
                text: "SECOND RELAY IS LIVE. FABRICATOR HAS A CLEAN LINE TO THE VAULT.",
                trigger: DialogueTrigger::RelaysOnline(2),
            },
        ],
        reactor_position: Some(Vec2::new(430.0, 10.0)),
        fabricator_position: Vec2::new(-1_030.0, -160.0),
        player_spawns: vec![
            PlayerSpawn::new(UnitKind::Warden, Vec2::new(-890.0, -260.0), 175.0, 175.0),
            PlayerSpawn::new(UnitKind::Engineer, Vec2::new(-950.0, -340.0), 130.0, 150.0),
            PlayerSpawn::new(UnitKind::Surveyor, Vec2::new(-820.0, -390.0), 90.0, 210.0),
        ],
        enemy_spawns: vec![
            EnemySpawn::new(UnitKind::Needle, Vec2::new(-330.0, 300.0), 95.0, 130.0),
            EnemySpawn::new(UnitKind::BellMine, Vec2::new(-60.0, -260.0), 95.0, 80.0),
            EnemySpawn::new(UnitKind::Needle, Vec2::new(310.0, 220.0), 95.0, 130.0),
            EnemySpawn::new(UnitKind::Canticle, Vec2::new(560.0, 30.0), 380.0, 120.0),
        ],
        obstacles: vec![Aabb::from_center_size(
            Vec2::new(20.0, 480.0),
            Vec2::new(1_650.0, 80.0),
        )],
        lumen_console: None,
        victory: VictoryCondition::RestoreRelaysAndDefeatBoss {
            boss_kind: UnitKind::Canticle,
        },
        unlock_next: 5,
        reward_lumen: 100,
        unlock_decision: Some("meridian-allied"),
        required_tier: 4,
    }
}
