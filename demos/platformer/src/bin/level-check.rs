//! Level validator + bot solver CLI: exit 0 + a JSON verdict line.
//!
//! Used by the aurora-mcp `aurora_validate_level` and `aurora_level_author`
//! tools so agents and humans share one fail-closed validation path.
//!
//! Usage:
//!   level-check <path>            validate + compile
//!   level-check <path> --solve    also run the playthrough bot

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let solve = args.iter().any(|arg| arg == "--solve");
    let Some(path) = args.iter().find(|arg| !arg.starts_with("--")).cloned() else {
        eprintln!("usage: level-check <path-to-level.json> [--solve]");
        std::process::exit(2);
    };

    let level = match std::fs::read_to_string(&path) {
        Ok(json) => match aurora_engine::Level::from_json(&json) {
            Ok(level) => level,
            Err(error) => {
                fail(&format!("could not read {path}: invalid level: {error}"));
            }
        },
        Err(error) => fail(&format!("could not read {path}: {error}")),
    };

    if !solve {
        println!(
            "{{\"valid\":true,\"id\":{},\"name\":{},\"pickups\":{},\"checkpoints\":{},\"movers\":{}}}",
            serde_json::to_string(&level.id).unwrap(),
            serde_json::to_string(&level.name).unwrap(),
            level.pickups.len(),
            level.checkpoints.len(),
            level.movers.len(),
        );
        return;
    }

    let json = std::fs::read_to_string(&path).expect("read succeeded above");
    match platformer::game_core::playthrough::solve(&json) {
        Ok(result) => {
            println!(
                "{{\"valid\":true,\"solvable\":{},\"id\":{},\"ticks\":{},\"collected\":{},\"total\":{},\"won\":{}}}",
                result.won,
                serde_json::to_string(&level.id).unwrap(),
                result.ticks_used,
                result.collected,
                result.total_pickups,
                result.won,
            );
            if !result.won {
                std::process::exit(1);
            }
        }
        Err(error) => fail(&format!("bot could not run the level: {error}")),
    }
}

fn fail(message: &str) -> ! {
    println!(
        "{{\"valid\":false,\"error\":{}}}",
        serde_json::to_string(message).unwrap()
    );
    std::process::exit(1);
}
