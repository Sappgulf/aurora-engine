# Aurora Engine

A Rust game engine for deterministic 2D strategy and action games, built with wgpu for desktop and WebGPU browsers.

Aurora's flagship is **Aurora: Last Light**, a campaign RTS about restoring power, directing specialist squads, and surviving the Choir. A five-level platformer and smaller demos exercise the same engine's physics, rendering, and playtesting tools.

[Project guide](PROJECT_GUIDE.md) · [Campaign bible](docs/AURORA_LAST_LIGHT_CAMPAIGN.md) · [Roadmap](ROADMAP.md)

## Explore the project

- **Last Light:** seven authored missions, tactical selection and queued orders, resource collection, powered structures, and persistent specialist loadouts.
- **Platformer:** five JSON-authored levels with moving platforms, checkpoints, dash, ghost racing, replay capture, and a boss arena.
- **Shared engine:** fixed-step simulation, semantic command traces, state hashing, 2D lighting, sprite batching, audio, gamepad input, and native/browser saves.
- **Optional 3D path:** glTF loading, a depth-tested PBR mesh pipeline, directional shadows, and sky rendering behind the `3d` feature.
- **Playtesting tools:** renderer-free simulations, Playwright browser scenarios, and an opt-in native agent control plane.

This is an engine in active development. Feature descriptions reflect the source tree; they are not a claim that every platform or release gate has passed.

## Run locally

Install Rust and Cargo, clone the repository, then run either showcase:

```bash
git clone https://github.com/Sappgulf/aurora-engine.git
cd aurora-engine
cargo run -p last_light
cargo run -p platformer
```

For Last Light, select a mission with the arrow keys and press **Space / Enter**. Left-click or drag to select units; right-click to issue an order. In the platformer, choose a level with **A/D** or the arrow keys, press **Space / Enter**, then move with **A/D** and jump with **Space**.

Full keyboard, controller, campaign, and replay controls are in the [project guide](PROJECT_GUIDE.md#controls).

## Run in a browser

Use a WebGPU-capable browser and install the Rust WASM target and Trunk:

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cd games/last-light
env -u NO_COLOR trunk serve
```

Open **http://127.0.0.1:8080**. For platformer builds and browser-test setup, see [development and validation](PROJECT_GUIDE.md#development-and-validation).

## Local release gate

Aurora uses a reproducible local gate rather than hosted GitHub Actions:

```bash
./scripts/validate-local.sh
```

The gate includes formatting, Clippy, workspace and deterministic tests, WASM checks, both web builds, browser playtests, and delivery budgets. Install the [validation prerequisites](PROJECT_GUIDE.md#development-and-validation) first. Passing a native check alone does not establish browser compatibility.

## Find your way around

| Path | Purpose |
| --- | --- |
| [crates/aurora-engine](crates/aurora-engine) | Reusable engine systems |
| [games/last-light](games/last-light) | Campaign, missions, simulation, and presentation |
| [demos/platformer](demos/platformer) | Data-driven action-game showcase |
| [demos](demos) | Rendering, skirmish, and engine contract demos |
| [playtests](playtests) | Browser scenarios and deterministic trace inputs |
| [tools/aurora-mcp](tools/aurora-mcp) | Bounded agent tooling and protocol documentation |

See the [asset production guide](docs/AURORA_LAST_LIGHT_ASSET_GUIDE.md) for Last Light art workflows.

## License

The repository includes [LICENSE-MIT](LICENSE-MIT). Workspace package metadata declares `MIT OR Apache-2.0`, but an Apache-2.0 license file is not included; that discrepancy remains to be reconciled.
