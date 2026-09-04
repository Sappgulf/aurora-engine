#!/usr/bin/env python3
"""Protocol tests for the Aurora MCP server and its agent-control helpers.

The stdio handshake section requires the ``mcp`` package (see
requirements.txt); it is skipped with a note when that package is missing.
The agent-control helper tests are dependency-free and run standalone in any
environment. Tests that would need the actual game running are not included.
"""

from __future__ import annotations

import asyncio
import socket
import sys
from pathlib import Path
from typing import Callable

ROOT = Path(__file__).resolve().parents[2]
TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

from agent_control import (  # noqa: E402
    AgentClient,
    AgentControlError,
    MOVE_THRESHOLD_UNITS,
    MAX_AGENT_FRAME_BYTES,
    add_step,
    build_transcript,
    free_port,
    movement_delta,
    next_request_id,
    response_matches,
)


def test_request_id_matching() -> None:
    assert next_request_id([]) == 1
    assert next_request_id({1, 2, 3}) == 4
    assert next_request_id({2}) == 1
    assert response_matches({"id": 2, "cmd": "state"}, {"id": 2, "ok": True, "result": {}})
    assert response_matches({"id": 2, "cmd": "state"}, {"id": 2, "ok": False, "error": "denied"})
    assert not response_matches({"id": 2, "cmd": "state"}, {"id": 3, "ok": True})
    assert not response_matches({"id": 2, "cmd": "state"}, {"id": 2, "result": {}})
    assert not response_matches({"id": 2, "cmd": "state"}, "not-a-dict")
    assert not response_matches({"cmd": "ping"}, {"id": 1, "ok": True})


def test_free_port_is_bindable() -> None:
    port = free_port()
    assert 0 < port < 65536
    assert 20_000 <= port <= 40_000  # preferred agent range
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", port))
        assert sock.getsockname()[1] == port


def test_transcript_building() -> None:
    steps: list[dict] = []
    add_step(steps, "ping", request={"id": 1, "cmd": "ping"}, response={"id": 1, "ok": True})
    add_step(
        steps,
        "state",
        request={"id": 2, "cmd": "state"},
        response={"id": 2, "ok": True, "result": {"screen": "playing"}},
    )
    transcript = build_transcript(
        steps,
        start_position=[100.0, 0.0],
        end_state={
            "screen": "playing",
            "position": [145.0, 0.0],
            "collected": 2,
            "hash": "18446744073709551615",
        },
        screenshot_path="/tmp/aurora-agent.png",
    )
    assert transcript["moved"] is True
    assert transcript["dx"] >= MOVE_THRESHOLD_UNITS
    assert transcript["dx"] == 45.0
    assert transcript["screen"] == "playing"
    assert transcript["collected"] == 2
    assert transcript["hash"] == "18446744073709551615"
    assert transcript["screenshot"] == "/tmp/aurora-agent.png"
    assert [step["description"] for step in transcript["steps"]] == ["ping", "state"]

    stationary = build_transcript(
        [],
        start_position=[10.0, 0.0],
        end_state={"screen": "playing", "position": [10.5, 0.0]},
        screenshot_path="x.png",
    )
    assert stationary["moved"] is False
    assert stationary["steps"] == []
    assert movement_delta(None, [1.0, 2.0]) == 0.0
    assert movement_delta([3.0, 1.0], None) == 0.0


def test_save_name_sanitizer() -> None:
    try:
        from aurora_mcp import _sanitize_save_name
    except ModuleNotFoundError as exc:
        if exc.name == "mcp":
            print("  (skipped: importing aurora_mcp requires the mcp package)")
            return
        raise
    assert _sanitize_save_name("  My Cool Level!  ") == "my-cool-level"
    assert _sanitize_save_name("Crystal_Run.v2") == "crystal-run-v2"
    assert _sanitize_save_name("a/b\\c") == "a-b-c"
    assert _sanitize_save_name("--Deep--Dive--") == "deep-dive"
    assert len(_sanitize_save_name("x" * 80)) == 40  # overlong names truncate, never pass through
    assert _sanitize_save_name("x" * 41) == "x" * 40
    for bad in ("ab", "--", "...", "", "a ", "!!"):
        try:
            _sanitize_save_name(bad)
        except ValueError:
            continue
        raise AssertionError(f"expected ValueError for {bad!r}")


def test_agent_client_rejects_oversized_frames() -> None:
    class Sink:
        def write(self, _payload: bytes) -> None:
            raise AssertionError("oversized payload must not reach the writer")

        def flush(self) -> None:
            raise AssertionError("oversized payload must not flush")

    client = AgentClient()
    client._writer = Sink()
    try:
        client.send({"id": 1, "cmd": "game", "action": "x" * MAX_AGENT_FRAME_BYTES})
    except AgentControlError as exc:
        assert "maximum" in str(exc)
    else:
        raise AssertionError("expected an oversized frame to be rejected")


async def mcp_stdio_handshake() -> None:
    from mcp import ClientSession, StdioServerParameters
    from mcp.client.stdio import stdio_client

    params = StdioServerParameters(command=sys.executable, args=["tools/aurora-mcp/aurora_mcp.py"], cwd=str(ROOT))
    async with stdio_client(params) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            tools = await session.list_tools()
            names = {tool.name for tool in tools.tools}
            expected = {
                "aurora_get_overview",
                "aurora_list_systems",
                "aurora_read_source",
                "aurora_get_playtest_contract",
                "aurora_get_scenario_report",
                "aurora_run_validation",
                "aurora_playtest_platformer",
                "aurora_validate_level",
                "aurora_level_author",
                "aurora_evidence_gallery",
                "aurora_agent_control",
                "aurora_agent_scenario",
            }
            assert len(tools.tools) == 12, f"Expected 12 registered tools, found {len(tools.tools)}: {sorted(names)}"
            missing = expected - names
            assert not missing, f"Missing expected MCP tools: {sorted(missing)}"

            resources = await session.list_resources()
            uris = {str(resource.uri) for resource in resources.resources}
            assert {"aurora://overview", "aurora://playtest-contract"} <= uris

            overview = await session.call_tool("aurora_get_overview", {"response_format": "json"})
            assert not overview.isError, "Overview tool returned an error"
            assert "Aurora" in overview.content[0].text

            rejected = await session.call_tool(
                "aurora_read_source", {"source": "../../etc/passwd", "response_format": "markdown"}
            )
            assert rejected.isError or "Error:" in rejected.content[0].text

            scenario = await session.call_tool(
                "aurora_get_scenario_report",
                {"scenario_id": "last_light.reclaim.relay_production", "response_format": "json"},
            )
            assert not scenario.isError, "Approved scenario report returned an error"
            assert '"end_tick": 3600' in scenario.content[0].text
            assert '"command_count": 8' in scenario.content[0].text

            gallery = await session.call_tool("aurora_evidence_gallery", {"response_format": "json"})
            assert not gallery.isError, "Evidence gallery returned an error"
            assert '"directories"' in gallery.content[0].text

            bad_level = await session.call_tool("aurora_level_author", {"level_json": "not-json"})
            assert bad_level.isError or "Error:" in bad_level.content[0].text


def main() -> None:
    helper_tests: list[tuple[str, Callable[[], None]]] = [
        ("agent request id matching", test_request_id_matching),
        ("agent free_port bindable", test_free_port_is_bindable),
        ("agent transcript building", test_transcript_building),
        ("agent client frame limit", test_agent_client_rejects_oversized_frames),
        ("level author save-name sanitizer", test_save_name_sanitizer),
    ]
    failed: list[str] = []
    for name, check in helper_tests:
        try:
            check()
        except Exception as exc:
            failed.append(f"{name}: {exc}")
            print(f"FAIL {name}: {exc}")
        else:
            print(f"ok   {name}")

    try:
        import mcp  # noqa: F401
    except ModuleNotFoundError:
        print(
            "skip MCP stdio handshake: the mcp package is not installed "
            "(pip install -r tools/aurora-mcp/requirements.txt)"
        )
    else:
        try:
            asyncio.run(mcp_stdio_handshake())
        except Exception as exc:
            failed.append(f"mcp stdio handshake: {exc}")
            print(f"FAIL mcp stdio handshake: {exc}")
        else:
            print("ok   mcp stdio handshake")

    if failed:
        raise SystemExit(1)
    print("All protocol tests passed")


if __name__ == "__main__":
    main()
