#!/usr/bin/env python3
"""Minimal stdio protocol smoke test for aurora_mcp.py; requires mcp dependencies."""

from __future__ import annotations

import asyncio
from pathlib import Path
import sys

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client


ROOT = Path(__file__).resolve().parents[2]


async def main() -> None:
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
                "aurora_run_validation",
            }
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

    print("MCP stdio smoke test passed")


if __name__ == "__main__":
    asyncio.run(main())
