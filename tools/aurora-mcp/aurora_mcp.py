#!/usr/bin/env python3
"""Aurora Engine's local, repository-scoped MCP server.

The server deliberately exposes a small, safe workflow for models working on
this repository.  It never accepts a shell command or an arbitrary path.  The
only potentially mutating operation is an explicitly selected validation lane,
which is limited to fixed Cargo commands and is clearly annotated as such.

Install the dependencies in ``requirements.txt`` and start it with:

    python3 tools/aurora-mcp/aurora_mcp.py

Use ``AURORA_ENGINE_ROOT`` only when the server lives outside of the checkout.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import subprocess
import sys
from enum import Enum
from pathlib import Path
from typing import Annotated, Literal

from mcp.server.fastmcp import FastMCP
from pydantic import Field


SERVER_NAME = "aurora_engine_mcp"
CHARACTER_LIMIT = 16_000
MAX_SOURCE_LINES = 400
LOGGER = logging.getLogger(SERVER_NAME)


def _repo_root() -> Path:
    """Resolve and validate the one repository this server may inspect."""
    configured = os.environ.get("AURORA_ENGINE_ROOT")
    root = Path(configured).expanduser().resolve() if configured else Path(__file__).resolve().parents[2]
    if not (root / "Cargo.toml").is_file() or not (root / "crates" / "aurora-engine").is_dir():
        raise RuntimeError(
            "Aurora Engine repository not found. Start from the checkout or set "
            "AURORA_ENGINE_ROOT to its root."
        )
    return root


REPO_ROOT = _repo_root()

# A closed allow-list makes source inspection useful without turning this into a
# local file browser.  The id, rather than a user-provided path, is the contract.
SOURCE_MAP: dict[str, tuple[str, str]] = {
    "readme": ("README.md", "Project commands and current feature summary."),
    "roadmap": ("ROADMAP.md", "Planned engine milestones."),
    "engine_app": ("crates/aurora-engine/src/app.rs", "Application loop and Game callbacks."),
    "engine_renderer": ("crates/aurora-engine/src/renderer.rs", "wgpu sprite renderer and render targets."),
    "engine_camera": ("crates/aurora-engine/src/camera.rs", "2D camera projection and viewport helpers."),
    "engine_input": ("crates/aurora-engine/src/input.rs", "Frame input and game-owned semantic bindings."),
    "engine_scene": ("crates/aurora-engine/src/scene.rs", "Generation-safe scene/entity storage."),
    "engine_tilemap": ("crates/aurora-engine/src/tilemap.rs", "Tile layers, solid tiles, and triggers."),
    "engine_audio": ("crates/aurora-engine/src/audio.rs", "Audio mixer and sound cues."),
    "engine_save": ("crates/aurora-engine/src/save.rs", "Typed portable storage and versioned save envelopes."),
    "engine_diagnostics": ("crates/aurora-engine/src/diagnostics.rs", "Frame timing and render diagnostics."),
    "engine_rts": ("crates/aurora-engine/src/rts.rs", "RTS orders, economy, production, power, navigation, and fog."),
    "engine_trace": (
        "crates/aurora-engine/src/trace.rs",
        "Deterministic semantic traces, state hashes, and bounded headless replay.",
    ),
    "engine_3d": ("crates/aurora-engine/src/mesh3d.rs", "Feature-gated mesh/material contracts."),
    "aurora_run": ("games/aurora-run/src/main.rs", "Playable Aurora Run vertical slice."),
    "last_light": ("games/last-light/src/main.rs", "Playable Last Light RTS campaign mission."),
    "last_light_simulation": (
        "games/last-light/src/simulation.rs",
        "Renderer-free Last Light roster, selection, navigation, movement, and trace state.",
    ),
    "last_light_campaign": ("docs/AURORA_LAST_LIGHT_CAMPAIGN.md", "Campaign, factions, characters, and mission arc."),
    "last_light_assets": ("docs/AURORA_LAST_LIGHT_ASSET_GUIDE.md", "Production asset and animation contract."),
}

VALIDATION_LANES: dict[str, tuple[str, ...]] = {
    "fast": ("cargo", "check", "--workspace"),
    "test": ("cargo", "test", "--workspace"),
    "web": ("cargo", "check", "--target", "wasm32-unknown-unknown", "-p", "last_light"),
}

SCENARIO_REPORTS: dict[str, tuple[str, str]] = {
    "last_light.reclaim.relay_production": (
        "playtests/last_light/reclaim_reactor_truth.aurora-trace",
        "reports/latest.json",
    ),
}


class ResponseFormat(str, Enum):
    """Supported output encodings for read-only tools."""

    MARKDOWN = "markdown"
    JSON = "json"


def _format(payload: object, response_format: ResponseFormat) -> str:
    """Render a compact bounded response without leaking unlimited file content."""
    if response_format is ResponseFormat.JSON:
        rendered = json.dumps(payload, indent=2, sort_keys=True)
    elif isinstance(payload, str):
        rendered = payload
    else:
        rendered = _as_markdown(payload)

    if len(rendered) <= CHARACTER_LIMIT:
        return rendered
    LOGGER.warning("Truncated MCP response from %s characters", len(rendered))
    return (
        rendered[: CHARACTER_LIMIT - 180]
        + "\n\n[Response truncated at 16,000 characters. Request a narrower source slice or a smaller page.]"
    )


def _as_markdown(payload: object) -> str:
    if not isinstance(payload, dict):
        return str(payload)
    title = str(payload.get("title", "Aurora Engine"))
    lines = [f"# {title}", ""]
    for key, value in payload.items():
        if key == "title":
            continue
        label = key.replace("_", " ").title()
        if isinstance(value, list):
            lines.append(f"## {label}")
            for item in value:
                if isinstance(item, dict):
                    heading = item.get("id") or item.get("name") or item.get("path") or "Item"
                    lines.append(f"- **{heading}** — " + ", ".join(
                        f"{name}: {val}" for name, val in item.items() if name not in {"id", "name", "path"}
                    ))
                else:
                    lines.append(f"- {item}")
            lines.append("")
        elif isinstance(value, dict):
            lines.append(f"## {label}")
            lines.extend(f"- **{name}**: `{val}`" for name, val in value.items())
            lines.append("")
        else:
            lines.append(f"- **{label}**: {value}")
    return "\n".join(lines).rstrip()


def _git_value(*args: str) -> str:
    """Run a fixed, read-only git query scoped to the repository."""
    try:
        completed = subprocess.run(
            ["git", "-C", str(REPO_ROOT), *args],
            check=True,
            capture_output=True,
            text=True,
            timeout=5,
        )
        return completed.stdout.strip()
    except (OSError, subprocess.SubprocessError) as exc:
        LOGGER.warning("Git metadata unavailable: %s", type(exc).__name__)
        return "unavailable"


def _source_path(source: str) -> Path:
    """Return an allow-listed source path after a defense-in-depth containment check."""
    try:
        relative, _ = SOURCE_MAP[source]
    except KeyError as exc:
        choices = ", ".join(sorted(SOURCE_MAP))
        raise ValueError(f"Unknown source '{source}'. Choose one of: {choices}.") from exc
    candidate = (REPO_ROOT / relative).resolve()
    if REPO_ROOT not in candidate.parents or not candidate.is_file():
        raise ValueError("That approved source is unavailable in this checkout. Use aurora_list_systems first.")
    return candidate


def _system_records() -> list[dict[str, str]]:
    """Build an overview from stable, approved source locations."""
    records: list[dict[str, str]] = []
    for system_id, (relative, description) in SOURCE_MAP.items():
        if system_id in {"readme", "roadmap", "aurora_run", "last_light", "last_light_campaign", "last_light_assets"}:
            continue
        records.append(
            {
                "id": system_id.removeprefix("engine_"),
                "path": relative,
                "description": description,
                "available": str((REPO_ROOT / relative).is_file()).lower(),
            }
        )
    return records


mcp = FastMCP(SERVER_NAME)


@mcp.resource("aurora://overview")
def aurora_overview_resource() -> str:
    """A compact current repository overview for MCP clients that support resources."""
    return _format(_overview_payload(), ResponseFormat.MARKDOWN)


@mcp.resource("aurora://playtest-contract")
def aurora_playtest_resource() -> str:
    """Controls, run command, and deterministic validation lanes for Aurora Run."""
    return _format(_playtest_payload(), ResponseFormat.MARKDOWN)


def _overview_payload() -> dict[str, object]:
    return {
        "title": "Aurora Engine overview",
        "repository": REPO_ROOT.name,
        "branch": _git_value("branch", "--show-current"),
        "head": _git_value("rev-parse", "--short", "HEAD"),
        "working_tree": _git_value("status", "--short") or "clean",
        "default_game": "cargo run -p last_light",
        "systems": _system_records(),
    }


def _playtest_payload() -> dict[str, object]:
    return {
        "title": "Aurora: Last Light playtest contract",
        "run": "cargo run -p last_light",
        "controls": {
            "deploy": "Space or Enter closes the briefing",
            "select": "Left click or left-drag",
            "add_select": "Shift plus click or drag",
            "command": "Right click moves or attacks contextually",
            "production": "Q Warden, E Engineer, F Surveyor; H holds position",
            "construction": "B previews a field beacon; left click places; Esc cancels",
            "groups": "Command or Control plus 1-5 assigns; 1-5 recalls",
            "camera": "WASD or screen edge pans; wheel zooms; minimap click navigates",
            "upgrades": "During briefing: Z Field Optics, X Reactive Plating, C Fabricator Overclock",
            "loadouts": "During briefing: V cycles Ivo; N cycles Sena; selections save immediately",
            "doctrines": "During briefing: M cycles Mara; O cycles Olan; selections save immediately",
            "relationship": "After Lumen contact: L cycles Guardian or Witness protocol and saves",
            "alliances": "After their campaign decisions: P cycles Meridian; G cycles Verdant",
            "pause": "Esc",
        },
        "acceptance_checks": [
            "Briefing, tactical pause, victory, and defeat overlays protect the playfield.",
            "Point selection, drag selection, move, and attack orders visibly respond.",
            "Fog reveals around Lantern units while hidden Choir units remain concealed.",
            "Production spends salvage once, advances visibly, and deploys at the fabricator.",
            "Restored relays increase power and salvage income; control groups recall live units.",
            "Beacon previews distinguish powered, obstructed, and out-of-bounds positions.",
            "Minimap contacts respect fog and its camera rectangle moves after a minimap click.",
            "Campaign upgrades spend Lumen once and survive a native or browser reload.",
            "Engineer move, Surveyor scan, and Needle attack strips animate without checker backgrounds.",
            "Ivo and Sena loadout changes persist and alter relay, beacon, vision, or damage behavior.",
            "Mara and Olan doctrines persist and alter sustain, speed, relay income, or Choir damage.",
            "Canticle command and Bell Mine arming strips telegraph engagement before damage range.",
            "All six unit kinds play hit reactions and hold a final shutdown wreck frame.",
            "Lumen protocol stays locked before contact; Guardian or Witness persists after contact.",
            "Meridian and Verdant choices stay locked before their decisions and persist afterward.",
            "Bastion, Charter, Bloom, and Briar effects compose with existing doctrines.",
            "Victory persists mission completion and unlocks mission three without duplicate rewards.",
            "HUD remains anchored and the camera remains map-bounded after resize.",
        ],
        "validation_lanes": {name: " ".join(command) for name, command in VALIDATION_LANES.items()},
        "note": "Run validation only when the user has authorized build-artifact changes.",
    }


@mcp.tool(
    name="aurora_get_overview",
    annotations={"readOnlyHint": True, "destructiveHint": False, "idempotentHint": True, "openWorldHint": False},
)
async def aurora_get_overview(
    response_format: ResponseFormat = ResponseFormat.MARKDOWN,
) -> str:
    """Return the current Aurora checkout, git state, and engine subsystem map.

    Use this first to orient an implementation task. It only reads repository and
    git metadata; it never stages, edits, or sends data outside this machine.
    """
    return _format(_overview_payload(), response_format)


@mcp.tool(
    name="aurora_list_systems",
    annotations={"readOnlyHint": True, "destructiveHint": False, "idempotentHint": True, "openWorldHint": False},
)
async def aurora_list_systems(
    limit: Annotated[int, Field(ge=1, le=20, description="Number of systems to return, from 1 to 20.")] = 12,
    offset: Annotated[int, Field(ge=0, le=20, description="Number of systems to skip for pagination.")] = 0,
    response_format: ResponseFormat = ResponseFormat.MARKDOWN,
) -> str:
    """Page through the engine's approved subsystem map.

    Use the returned system ids with ``aurora_read_source``. This intentionally
    lists an engine-oriented map instead of every file in the checkout.
    """
    records = _system_records()
    items = records[offset : offset + limit]
    payload = {
        "title": "Aurora Engine systems",
        "total_count": len(records),
        "count": len(items),
        "offset": offset,
        "has_more": offset + len(items) < len(records),
        "next_offset": offset + len(items) if offset + len(items) < len(records) else None,
        "systems": items,
    }
    return _format(payload, response_format)


@mcp.tool(
    name="aurora_get_playtest_contract",
    annotations={"readOnlyHint": True, "destructiveHint": False, "idempotentHint": True, "openWorldHint": False},
)
async def aurora_get_playtest_contract(
    response_format: ResponseFormat = ResponseFormat.MARKDOWN,
) -> str:
    """Return how to run Last Light, its controls, and focused visual acceptance checks.

    Use before a playtest or before choosing an explicit validation lane. This is
    read-only and never launches the game.
    """
    return _format(_playtest_payload(), response_format)


@mcp.tool(
    name="aurora_get_scenario_report",
    annotations={"readOnlyHint": True, "destructiveHint": False, "idempotentHint": True, "openWorldHint": False},
)
async def aurora_get_scenario_report(
    scenario_id: Literal["last_light.reclaim.relay_production"],
    response_format: ResponseFormat = ResponseFormat.MARKDOWN,
) -> str:
    """Return the bounded checked-in trace and latest validation report for an approved scenario.

    ``scenario_id`` is a closed allow-list. The tool accepts no path, command,
    tick count, or executable input and never runs the scenario itself.
    """
    trace_relative, report_relative = SCENARIO_REPORTS[scenario_id]
    try:
        trace = json.loads((REPO_ROOT / trace_relative).read_text(encoding="utf-8"))
        report = json.loads((REPO_ROOT / report_relative).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        LOGGER.warning("Scenario evidence unavailable: %s", type(exc).__name__)
        return f"Error: checked-in evidence for '{scenario_id}' is unavailable or invalid."

    commands = trace.get("commands", [])
    if not isinstance(commands, list) or len(commands) > 64:
        return f"Error: checked-in trace for '{scenario_id}' exceeds the 64-command report bound."
    payload = {
        "title": "Aurora deterministic scenario report",
        "scenario_id": scenario_id,
        "available_scenarios": sorted(SCENARIO_REPORTS),
        "trace": {
            "path": trace_relative,
            "seed": trace.get("seed"),
            "fixed_tick_hz": trace.get("fixed_tick_hz"),
            "end_tick": trace.get("end_tick"),
            "command_count": len(commands),
            "actions": [command.get("action") for command in commands],
        },
        "validation": {
            "iteration": report.get("iteration"),
            "status": report.get("status"),
            "deterministic_replay_runs": report.get("evidence", {}).get("deterministic_replay_runs"),
            "determinism_mismatches": report.get("evidence", {}).get("determinism_mismatches"),
        },
    }
    return _format(payload, response_format)


@mcp.tool(
    name="aurora_read_source",
    annotations={"readOnlyHint": True, "destructiveHint": False, "idempotentHint": True, "openWorldHint": False},
)
async def aurora_read_source(
    source: Annotated[str, Field(min_length=3, max_length=40, description="An approved source id from aurora_list_systems, e.g. 'engine_renderer' or 'aurora_run'.")],
    start_line: Annotated[int, Field(ge=1, description="One-based first line to read.")] = 1,
    line_count: Annotated[int, Field(ge=1, le=MAX_SOURCE_LINES, description="Number of lines to read, from 1 to 400.")] = 160,
    response_format: ResponseFormat = ResponseFormat.MARKDOWN,
) -> str:
    """Read a bounded slice of one approved Aurora Engine source file.

    This is intentionally not an arbitrary file reader: ``source`` must be one
    of the documented ids, paths cannot be supplied, and each request is capped
    at 400 lines. Use ``aurora_list_systems`` to discover ids.
    """
    try:
        source_path = _source_path(source)
        lines = source_path.read_text(encoding="utf-8").splitlines()
    except (OSError, ValueError) as exc:
        return f"Error: {exc}"

    first = start_line - 1
    if first >= len(lines):
        return f"Error: start_line {start_line} is beyond {source}'s {len(lines)} lines. Try a smaller start_line."
    selected = lines[first : first + line_count]
    numbered = "\n".join(f"{index:>5} | {line}" for index, line in enumerate(selected, start=start_line))
    payload = {
        "title": f"{source} lines {start_line}-{start_line + len(selected) - 1}",
        "source": source,
        "path": str(source_path.relative_to(REPO_ROOT)),
        "total_lines": len(lines),
        "has_more": first + len(selected) < len(lines),
        "next_start_line": first + len(selected) + 1 if first + len(selected) < len(lines) else None,
        "content": numbered,
    }
    if response_format is ResponseFormat.JSON:
        return _format(payload, response_format)
    return _format(f"# {payload['title']}\n\n```text\n{numbered}\n```", response_format)


@mcp.tool(
    name="aurora_run_validation",
    annotations={"readOnlyHint": False, "destructiveHint": False, "idempotentHint": True, "openWorldHint": False},
)
async def aurora_run_validation(
    lane: Literal["fast", "test", "web"] = "fast",
) -> str:
    """Run one fixed, non-destructive Aurora validation lane after user approval.

    The only allowed lanes are ``fast`` (workspace check), ``test`` (workspace
    tests), and ``web`` (Last Light WASM check). The tool cannot run arbitrary
    commands, modify source, stage files, commit, or contact a remote. Cargo may
    create local build artifacts, so this operation is deliberately not marked
    read-only.
    """
    command = VALIDATION_LANES[lane]
    try:
        process = await asyncio.create_subprocess_exec(
            *command,
            cwd=REPO_ROOT,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.STDOUT,
        )
        stdout, _ = await asyncio.wait_for(process.communicate(), timeout=180)
    except asyncio.TimeoutError:
        return f"Error: validation lane '{lane}' exceeded 180 seconds and was stopped. Try the fast lane or inspect the local build."
    except OSError as exc:
        LOGGER.warning("Validation could not start: %s", type(exc).__name__)
        return "Error: Cargo could not start. Confirm Rust is installed and use aurora_get_playtest_contract for the command."

    output = stdout.decode("utf-8", errors="replace").strip()
    tail = output[-8_000:] if output else "(Cargo produced no output.)"
    status = "passed" if process.returncode == 0 else "failed"
    return _format(
        {
            "title": f"Validation {status}: {lane}",
            "command": " ".join(command),
            "exit_code": process.returncode,
            "output_tail": tail,
        },
        ResponseFormat.MARKDOWN,
    )


if __name__ == "__main__":
    logging.basicConfig(stream=sys.stderr, level=logging.INFO, format="%(name)s %(levelname)s: %(message)s")
    mcp.run(transport="stdio")
