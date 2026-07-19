# Aurora Engine MCP

`aurora_mcp.py` is a local stdio [Model Context Protocol](https://modelcontextprotocol.io/) server for working on this checkout. It gives an agent a compact, engine-specific orientation flow without granting a generic shell, arbitrary file reader, network access, or Git write access.

## Install and connect

Run these commands from the Aurora Engine root:

```bash
python3 -m venv .venv-mcp
.venv-mcp/bin/python -m pip install -r mcp/requirements.txt
```

Copy `mcp/config.example.json` into your MCP client's configuration and replace its placeholder `cwd` with this checkout's absolute path. When using the virtual environment, set `command` to the absolute path of `.venv-mcp/bin/python`; keep `args` as `['mcp/aurora_mcp.py']`.

For a standalone smoke check, use the provided protocol test after installing dependencies:

```bash
.venv-mcp/bin/python -m py_compile mcp/aurora_mcp.py
.venv-mcp/bin/python mcp/test_protocol.py
```

The server uses stdio. It must not print log messages to stdout, since stdout is reserved for MCP frames.

## Model workflow

1. Call `aurora_get_overview` to see the branch, working tree, core systems, and default game command.
2. Call `aurora_list_systems` and then `aurora_read_source` with an approved source id and a small line slice.
3. Call `aurora_get_playtest_contract` before a visual/gameplay pass.
4. Only after user authorization, call `aurora_run_validation` with a fixed lane.

## Tools and resources

| Capability | Purpose | Effects |
|---|---|---|
| `aurora_get_overview` | Repo/git orientation and system map | Read-only |
| `aurora_list_systems` | Paginated list of engine systems | Read-only |
| `aurora_read_source` | Bounded slice of a selected allow-listed source file | Read-only |
| `aurora_get_playtest_contract` | Run command, controls, and visual acceptance checks | Read-only |
| `aurora_run_validation` | One fixed Cargo lane: `fast`, `test`, or `web` | Creates Cargo build artifacts only |
| `aurora://overview` | Overview resource | Read-only |
| `aurora://playtest-contract` | Playtest resource | Read-only |

`aurora_read_source` accepts ids only, never arbitrary paths. A slice is capped at 400 lines and all rendered results are capped at 16,000 characters. `aurora_run_validation` accepts an enum, never a command string; it cannot stage, commit, push, alter source, or make network requests.

## Security and operating boundary

- The root defaults to the repository containing the server and must look like Aurora Engine. `AURORA_ENGINE_ROOT` is an explicit local override for a separate checkout.
- Git calls use fixed read-only argument lists. Validation uses fixed Cargo argument lists and a 180-second timeout.
- The server sends only MCP stdio traffic to its parent process; it has no HTTP client and no credential handling.
- Treat validation as user-approved work: Cargo writes to local `target/` even though it does not edit source.

## Evaluation and test coverage

`evals/read_only.xml` contains ten independent, read-only prompts that exercise overview, system discovery, bounded source reading, and the playtest contract. `test_protocol.py` starts the stdio server, performs the MCP initialize handshake, lists tools/resources, calls the read-only overview, and confirms traversal-like input is rejected.
