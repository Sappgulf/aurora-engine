# Aurora Engine MCP

`aurora_mcp.py` is a local stdio [Model Context Protocol](https://modelcontextprotocol.io/) server for working on this checkout. It gives an agent a compact, engine-specific orientation flow without granting a generic shell, arbitrary file reader, network access, or Git write access. The reusable client for the engine's agent runtime control plane lives beside it in `agent_control.py`.

## Install and connect

Run these commands from the Aurora Engine root:

```bash
python3 -m venv .venv-mcp
.venv-mcp/bin/python -m pip install -r tools/aurora-mcp/requirements.txt
```

Copy `tools/aurora-mcp/config.example.json` into your MCP client's configuration and replace its placeholder `cwd` with this checkout's absolute path. When using the virtual environment, set `command` to the absolute path of `.venv-mcp/bin/python`; keep `args` as `['tools/aurora-mcp/aurora_mcp.py']`.

For a standalone smoke check, use the provided protocol test after installing dependencies:

```bash
.venv-mcp/bin/python -m py_compile tools/aurora-mcp/aurora_mcp.py
.venv-mcp/bin/python tools/aurora-mcp/test_protocol.py
```

The server uses stdio. It must not print log messages to stdout, since stdout is reserved for MCP frames.

## Model workflow

1. Call `aurora_get_overview` to see the branch, working tree, core systems, and default game command.
2. Call `aurora_list_systems` and then `aurora_read_source` with an approved source id and a small line slice.
3. Call `aurora_get_playtest_contract` before a visual/gameplay pass.
4. Call `aurora_get_scenario_report` with an approved scenario id for bounded deterministic evidence.
5. Only after user authorization, call `aurora_run_validation` with a fixed lane.
6. When working on the platformer demo, call `aurora_validate_level` (raw JSON or a `demos/platformer/levels/` path) and `aurora_playtest_platformer` for its test lane.
7. Only for a dev playtest on a machine you control, call `aurora_agent_control` to launch the platformer with the loopback-only agent control plane and run one bounded start-and-move scenario.

## Tools and resources

| Capability | Purpose | Effects |
|---|---|---|
| `aurora_get_overview` | Repo/git orientation and system map | Read-only |
| `aurora_list_systems` | Paginated list of engine systems | Read-only |
| `aurora_read_source` | Bounded slice of a selected allow-listed source file | Read-only |
| `aurora_get_playtest_contract` | Run command, controls, and visual acceptance checks | Read-only |
| `aurora_get_scenario_report` | Bounded trace metadata and validation evidence for an allow-listed scenario id | Read-only |
| `aurora_run_validation` | One fixed Cargo lane: `fast`, `test`, or `web` | Creates Cargo build artifacts only |
| `aurora_playtest_platformer` | Fixed bounded lane running `cargo test -p platformer` | Creates Cargo build artifacts only |
| `aurora_validate_level` | Runs the fixed `level-check` binary on level JSON or an allow-listed `demos/platformer/levels/` path | Writes only a system temp file; creates Cargo build artifacts only |
| `aurora_level_author` | Validates authored level JSON and requires a bot-solve proof (`--solve`) before optionally persisting it as `demos/platformer/levels/<save_name>.json` | Writes one system temp file; persists into `demos/platformer/levels/` only when both stages pass; creates Cargo build artifacts only |
| `aurora_evidence_gallery` | Lists playtest screenshot PNGs from `playtests/screenshots/` and the system temp dir, newest first, capped at 50 | Read-only |
| `aurora_agent_scenario` | Run a custom agent-authored scenario: ordered protocol commands with waits and state polls, optionally attaching to an already-running game |
| `aurora_agent_control` | Launches the native platformer with the agent control plane and drives one bounded scenario | Starts and always terminates a local game process; writes one screenshot |
| `aurora://overview` | Overview resource | Read-only |
| `aurora://playtest-contract` | Playtest resource | Read-only |

`aurora_read_source` accepts ids only, never arbitrary paths. A slice is capped at 400 lines and all rendered results are capped at 16,000 characters. `aurora_get_scenario_report` accepts a closed scenario-id enum, caps traces at 64 commands, and never runs code. `aurora_run_validation` and `aurora_playtest_platformer` accept an enum or no arguments, never a command string; they cannot stage, commit, push, alter source, or make network requests. `aurora_validate_level` rejects absolute paths and any traversal outside `demos/platformer/levels/`, and stages raw JSON only under the system temp directory. `aurora_agent_control` is bounded end to end and never leaves a game process behind.

## Security and operating boundary

- The root defaults to the repository containing the server and must look like Aurora Engine. `AURORA_ENGINE_ROOT` is an explicit local override for a separate checkout.
- Git calls use fixed read-only argument lists. Validation, platformer playtests, level checks, and the agent-control launcher use fixed Cargo argument lists with bounded timeouts (180s, 240s, 120s, and 120s respectively).
- `aurora_validate_level` never writes inside the repository: raw level JSON is staged only under the system temp directory, and level paths are resolved and contained inside `demos/platformer/levels/` (no absolute paths, no `..`).
- The agent-control launcher kills the whole game process group (`start_new_session=True` plus `os.killpg`) in a `finally` block, so no game or child process survives the tool call.
- The server sends only MCP stdio traffic to its parent process; it has no HTTP client and no credential handling.
- Treat validation and agent-control runs as user-approved work: Cargo writes to local `target/` even though it does not edit source.

## Agent runtime control plane (dev-only)

`agent_control.py` implements the client half of the engine's agent runtime control plane. When the platformer demo is launched with `AURORA_AGENT_PORT=<port>` in its environment, the engine listens on `127.0.0.1:<port>` and speaks newline-delimited JSON: each request frame is one JSON object per line, and each response is one line with a matching `id`. The control plane exists only for that process lifetime, binds to loopback only, and is fully opt-in per launch — nothing listens when the environment variable is absent. It is a development and playtesting aid, not a runtime service.

### Protocol contract

Requests (`{"id": N, ...}`) and responses (`{"id": N, "ok": true, "result": ...}` or `{"id": N, "ok": false, "error": "..."}`):

| Command | Example request | Notes |
|---|---|---|
| `ping` | `{"id": 1, "cmd": "ping"}` | Liveness probe |
| `state` | `{"id": 2, "cmd": "state"}` | Returns the platformer state snapshot below |
| `inject_key` | `{"id": 3, "cmd": "inject_key", "key": "KeyD", "down": true}` | Synthetic keyboard input |
| `inject_pad_button` | `{"id": 4, "cmd": "inject_pad_button", "button": "South", "down": true, "slot": 0}` | Synthetic gamepad input |
| `inject_pad_stick` | `{"id": 5, "cmd": "inject_pad_stick", "stick": "Right", "x": 1, "y": 0, "slot": 0}` | Synthetic analog stick sample |
| `screenshot` | `{"id": 6, "cmd": "screenshot", "path": "/abs/path.png"}` | Writes a PNG to an absolute path |
| `game` | `{"id": 7, "cmd": "game", "action": "reset"\|"load_level"\|"teleport", "args": {}}` | Direct game actions |

The `state` result for the platformer:

```json
{
  "screen": "level_select" | "playing",
  "level": "crystal-run",
  "level_index": 0,
  "position": [x, y],
  "velocity": [vx, vy],
  "on_ground": true,
  "collected": 0,
  "total": 3,
  "elapsed": 12.5,
  "ticks": 750,
  "won": false,
  "hash": "u64 state hash"
}
```

### Client API surface

`AgentClient` (loopback-only, every wait bounded): `wait_for_port(port, timeout_s)`, `send(request)`, `recv_response(timeout_s)`, `call(request) -> response` (matches response `id` to request `id`), `close()`. `drive_platformer(port, screenshot_path, max_seconds=20)` runs the bounded start-and-move scenario: connect, ping, read state, start the level from `level_select` when needed, hold `KeyD` until `position[0]` advances at least 40 units, release the key, capture a screenshot, and return a transcript. `free_port()` returns a loopback port that is free right now, preferring 20000-40000. Every send/recv is recorded in the transcript's `steps`.

### Example `aurora_agent_control` response

```json
{
  "steps": [
    {"step": 1, "description": "connected to the agent control plane", "request": {"port": 23451}},
    {"step": 2, "description": "ping", "request": {"id": 1, "cmd": "ping"}, "response": {"id": 1, "ok": true, "result": "pong"}},
    {"step": 3, "description": "state", "response": {"id": 2, "ok": true, "result": {"screen": "level_select"}}}
  ],
  "moved": true,
  "dx": 231.5,
  "screen": "playing",
  "collected": 0,
  "hash": "14098263128477120",
  "screenshot": "/tmp/aurora-mcp/platformer-agent.png"
}
```

Security boundaries: the game process is started locally by the MCP tool with `AURORA_AGENT_PORT` set; the client connects only to `127.0.0.1`; the scenario, its inputs, and its timeouts are fixed (no arbitrary commands or key sequences are accepted); and the screenshot defaults to the system temp directory.

## Evaluation and test coverage

`evals/read_only.xml` contains ten independent, read-only prompts that exercise overview, system discovery, bounded source reading, and the playtest contract. `test_protocol.py` has two layers: dependency-free unit tests for the agent-control helpers (request id matching, `free_port` bindability, transcript building) that run in any Python environment, and the stdio smoke test — which requires the `mcp` package from `requirements.txt` and is skipped with a note when it is missing — that starts the stdio server, performs the MCP initialize handshake, lists tools/resources, calls the read-only overview, and confirms traversal-like input is rejected. Tests that would need the actual game process running are intentionally not included.
