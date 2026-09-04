#!/usr/bin/env python3
"""Reusable, loopback-only client for the Aurora engine agent runtime control plane.

When the platformer demo is launched with ``AURORA_AGENT_PORT`` set to a port
number, the engine listens on ``127.0.0.1:<port>`` and speaks newline-delimited
JSON. This module implements that client plus one bounded scenario driver.

Design rules mirror the MCP server that imports it: the host is pinned to the
loopback interface, every wait is bounded by an explicit deadline so an
unresponsive game can never hang a caller, and only protocol messages from the
documented contract are ever produced. This is a development-only aid; the
control plane exists solely when a game process opts in via the environment
variable and never listens beyond 127.0.0.1.
"""

from __future__ import annotations

import json
import random
import socket
import time
from pathlib import Path
from typing import Any, Iterable, Sequence

HOST = "127.0.0.1"
DEFAULT_TIMEOUT_S = 5.0
POLL_INTERVAL_S = 0.1
CONNECT_RETRY_INTERVAL_S = 0.25
MOVE_THRESHOLD_UNITS = 40.0
MAX_STEPS = 512
MAX_AGENT_FRAME_BYTES = 64 * 1024
PREFERRED_PORT_LOW = 20_000
PREFERRED_PORT_HIGH = 40_000


class AgentControlError(RuntimeError):
    """Raised when a bounded agent-control operation cannot complete."""


def free_port(prefer_low: int = PREFERRED_PORT_LOW, prefer_high: int = PREFERRED_PORT_HIGH) -> int:
    """Return a loopback port that is free right now, preferring 20000-40000.

    The socket is closed before returning, so callers should treat the result
    as a race window they own: they must bind it immediately (which is exactly
    what launching the game with ``AURORA_AGENT_PORT`` does).
    """
    span = prefer_high - prefer_low
    for _ in range(32):
        candidate = random.randint(prefer_low, prefer_high) if span > 0 else 0
        if _port_bindable(candidate):
            return candidate
    return _ephemeral_port()


def _port_bindable(port: int) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        try:
            probe.bind((HOST, port))
        except OSError:
            return False
    return True


def _ephemeral_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind((HOST, 0))
        return int(probe.getsockname()[1])


def next_request_id(existing_ids: Iterable[int]) -> int:
    """Return the smallest free one-based protocol id not already in use."""
    used = {int(value) for value in existing_ids}
    candidate = 1
    while candidate in used:
        candidate += 1
    return candidate


def response_matches(request: dict[str, Any], response: Any) -> bool:
    """True when ``response`` answers ``request``: same id and a bool verdict."""
    if not isinstance(response, dict) or not isinstance(request, dict):
        return False
    if "id" not in request or "id" not in response:
        return False
    return response["id"] == request["id"] and isinstance(response.get("ok"), bool)


def add_step(
    steps: list[dict[str, Any]],
    description: str,
    *,
    request: dict[str, Any] | None = None,
    response: dict[str, Any] | None = None,
) -> dict[str, Any] | None:
    """Append one transcript entry and return it, or None once the cap is hit."""
    if len(steps) >= MAX_STEPS:
        return None
    entry: dict[str, Any] = {"step": len(steps) + 1, "description": str(description)}
    if request is not None:
        entry["request"] = request
    if response is not None:
        entry["response"] = response
    steps.append(entry)
    return entry


def movement_delta(start_position: Sequence[float] | None, end_position: Sequence[float] | None) -> float:
    """Signed horizontal delta between two ``[x, y]`` state positions."""
    if not start_position or not end_position:
        return 0.0
    return float(end_position[0]) - float(start_position[0])


def build_transcript(
    steps: list[dict[str, Any]],
    *,
    start_position: Sequence[float] | None,
    end_state: dict[str, Any],
    screenshot_path: str | Path,
    threshold: float = MOVE_THRESHOLD_UNITS,
) -> dict[str, Any]:
    """Assemble the bounded scenario transcript from recorded steps and final state."""
    dx = round(movement_delta(start_position, end_state.get("position")), 3)
    return {
        "steps": steps,
        "moved": dx >= threshold,
        "dx": dx,
        "screen": end_state.get("screen"),
        "collected": end_state.get("collected"),
        "hash": end_state.get("hash"),
        "screenshot": str(screenshot_path),
    }


class AgentClient:
    """A bounded, loopback-only JSON-lines client for the engine agent server."""

    def __init__(self, host: str = HOST) -> None:
        if host not in {HOST, "localhost"}:
            raise ValueError("The agent client may only connect to the loopback interface.")
        self._host = HOST
        self._sock: socket.socket | None = None
        self._reader: Any = None
        self._writer: Any = None
        self._used_ids: set[int] = set()

    def wait_for_port(self, port: int, timeout_s: float = DEFAULT_TIMEOUT_S) -> None:
        """Connect to 127.0.0.1:port, retrying until the bounded deadline expires."""
        deadline = time.monotonic() + max(timeout_s, 0.0)
        last_error: OSError | None = None
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise AgentControlError(
                    f"agent server on {self._host}:{port} did not open within {timeout_s:.1f}s"
                    + (f" (last error: {last_error})" if last_error else "")
                )
            sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            sock.settimeout(min(remaining, CONNECT_RETRY_INTERVAL_S))
            try:
                sock.connect((self._host, port))
            except OSError as exc:
                last_error = exc
                sock.close()
                time.sleep(min(CONNECT_RETRY_INTERVAL_S, remaining))
                continue
            self._sock = sock
            self._reader = sock.makefile("rb")
            self._writer = sock.makefile("wb")
            return

    def send(self, request: dict[str, Any]) -> None:
        """Write one newline-delimited JSON request frame."""
        if self._writer is None:
            raise AgentControlError("agent client is not connected; call wait_for_port first")
        frame = json.dumps(request, separators=(",", ":")) + "\n"
        encoded = frame.encode("utf-8")
        if len(encoded) - 1 > MAX_AGENT_FRAME_BYTES:
            raise AgentControlError(
                f"agent frame exceeds maximum of {MAX_AGENT_FRAME_BYTES} bytes"
            )
        try:
            self._writer.write(encoded)
            self._writer.flush()
        except (OSError, ValueError) as exc:
            raise AgentControlError(f"failed to send request to the agent server: {exc}") from exc

    def recv_response(self, timeout_s: float = DEFAULT_TIMEOUT_S) -> dict[str, Any]:
        """Read the next well-formed response frame, bounded by ``timeout_s``."""
        if self._reader is None or self._sock is None:
            raise AgentControlError("agent client is not connected; call wait_for_port first")
        deadline = time.monotonic() + max(timeout_s, 0.0)
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise AgentControlError(f"timed out after {timeout_s:.1f}s waiting for an agent response")
            self._sock.settimeout(remaining)
            try:
                line = self._reader.readline()
            except TimeoutError as exc:
                raise AgentControlError(f"timed out after {timeout_s:.1f}s waiting for an agent response") from exc
            except OSError as exc:
                raise AgentControlError(f"agent connection failed while reading: {exc}") from exc
            if not line:
                raise AgentControlError("the agent server closed the connection")
            try:
                response = json.loads(line)
            except json.JSONDecodeError as exc:
                raise AgentControlError(f"agent server sent a non-JSON frame: {line[:120]!r}") from exc
            if not isinstance(response, dict) or "id" not in response:
                continue
            return response

    def call(self, request: dict[str, Any], timeout_s: float = DEFAULT_TIMEOUT_S) -> dict[str, Any]:
        """Send one request and return the response whose id matches it."""
        payload = dict(request)
        if "id" not in payload:
            payload["id"] = next_request_id(self._used_ids)
        self._used_ids.add(int(payload["id"]))
        self.send(payload)
        deadline = time.monotonic() + max(timeout_s, 0.0)
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise AgentControlError(f"no agent response with id {payload['id']} within {timeout_s:.1f}s")
            response = self.recv_response(remaining)
            if response_matches(payload, response):
                return response

    def close(self) -> None:
        """Release the socket and its file wrappers; safe to call twice."""
        for handle in (self._writer, self._reader):
            if handle is not None:
                try:
                    handle.close()
                except OSError:
                    pass
        if self._sock is not None:
            try:
                self._sock.close()
            except OSError:
                pass
        self._sock = None
        self._reader = None
        self._writer = None


def drive_platformer(port: int, screenshot_path: str | Path, max_seconds: float = 20.0) -> dict[str, Any]:
    """Run the bounded start-and-move scenario against one running platformer.

    Connects to the agent control plane on ``127.0.0.1:port``, pings it, starts
    the level from the level-select screen when needed, holds ``KeyD`` until the
    player moves at least :data:`MOVE_THRESHOLD_UNITS` to the right, releases the
    key, captures a screenshot, and returns a transcript. Every wait is bounded
    by ``max_seconds`` overall and the key is always released in a finally block,
    so the scenario can never hang or leave input latched.
    """
    deadline = time.monotonic() + max(max_seconds, 0.0)
    screenshot = Path(screenshot_path)
    steps: list[dict[str, Any]] = []
    client = AgentClient()
    key_held = False
    start_position: list[float] | None = None
    end_state: dict[str, Any] = {}

    def remaining() -> float:
        left = deadline - time.monotonic()
        if left <= 0:
            raise AgentControlError(f"scenario exceeded its {max_seconds:.1f}s overall budget")
        return left

    def issue(command: dict[str, Any]) -> dict[str, Any]:
        payload = {"id": next_request_id(client._used_ids), **command}
        response = client.call(payload, timeout_s=remaining())
        add_step(steps, str(payload.get("cmd", "request")), request=payload, response=response)
        if not response.get("ok"):
            raise AgentControlError(
                f"agent command '{payload.get('cmd')}' failed: {response.get('error', 'unknown error')}"
            )
        return response

    def poll_state(description: str, predicate) -> dict[str, Any]:
        while True:
            left = remaining()
            response = issue({"cmd": "state"})
            state = response.get("result") or {}
            if predicate(state):
                return state
            time.sleep(min(POLL_INTERVAL_S, left))

    try:
        client.wait_for_port(port, timeout_s=remaining())
        add_step(steps, "connected to the agent control plane", request={"port": port})

        issue({"cmd": "ping"})
        # The agent server opens at engine startup, well before the render
        # loop runs. Wait until the frame counter actually advances before
        # injecting anything, or the first frame drains the press before the
        # window/game loop is ready to honor it.
        for _ in range(120):
            ping = issue({"cmd": "ping"})
            frame = (ping.get("result") or {}).get("frame", 0)
            if frame > 30:
                add_step(steps, "render loop running", request={"frame": frame})
                break
            time.sleep(min(0.1, remaining()))
        else:
            raise AgentControlError("game render loop never started (frame stuck near 0)")

        state = (issue({"cmd": "state"}).get("result") or {})
        if state.get("screen") == "level_select":
            issue({"cmd": "inject_key", "key": "Space", "down": True})
            issue({"cmd": "inject_key", "key": "Space", "down": False})
            state = poll_state("level to start", lambda s: s.get("screen") == "playing")
        if state.get("screen") != "playing":
            raise AgentControlError(f"game is on unexpected screen '{state.get('screen')}'")

        start_position = list(state.get("position") or [])
        issue({"cmd": "inject_key", "key": "KeyD", "down": True})
        key_held = True
        end_state = poll_state(
            "player to move right",
            lambda s: movement_delta(start_position, s.get("position")) >= MOVE_THRESHOLD_UNITS,
        )
        issue({"cmd": "inject_key", "key": "KeyD", "down": False})
        key_held = False

        try:
            screenshot.parent.mkdir(parents=True, exist_ok=True)
        except OSError as exc:
            raise AgentControlError(f"cannot prepare screenshot directory {screenshot.parent}: {exc}") from exc
        issue({"cmd": "screenshot", "path": str(screenshot)})
        end_state = (issue({"cmd": "state"}).get("result") or {})

        return build_transcript(
            steps,
            start_position=start_position,
            end_state=end_state,
            screenshot_path=str(screenshot),
        )
    finally:
        if key_held:
            try:
                client.send({"cmd": "inject_key", "key": "KeyD", "down": False})
            except AgentControlError:
                pass
        client.close()


MAX_SCENARIO_COMMANDS = 64

# Allowed keys per scenario step; anything else is rejected up front so a
# typo'd plan fails fast instead of half-running against a live game.
_SCENARIO_STEP_KEYS = {
    "cmd",
    "key",
    "down",
    "slot",
    "button",
    "stick",
    "x",
    "y",
    "delta",
    "path",
    "action",
    "args",
    "wait_seconds",
    "poll_state",
    "poll_timeout_s",
}


def validate_scenario(commands: Sequence[dict[str, Any]]) -> list[dict[str, Any]]:
    """Validates a scenario plan; returns it unchanged when well-formed."""
    if not isinstance(commands, list) or not commands:
        raise AgentControlError("scenario must be a non-empty list of step objects")
    if len(commands) > MAX_SCENARIO_COMMANDS:
        raise AgentControlError(f"scenario exceeds {MAX_SCENARIO_COMMANDS} steps")
    for index, step in enumerate(commands):
        if not isinstance(step, dict) or not step:
            raise AgentControlError(f"scenario step {index} must be a non-empty object")
        unknown = set(step) - _SCENARIO_STEP_KEYS
        if unknown:
            raise AgentControlError(
                f"scenario step {index} has unsupported keys: {sorted(unknown)}"
            )
        if "poll_state" in step and not isinstance(step["poll_state"], dict):
            raise AgentControlError(f"scenario step {index} poll_state must be an object")
        if "wait_seconds" in step:
            wait = step["wait_seconds"]
            if not isinstance(wait, (int, float)) or not 0 <= float(wait) <= 30:
                raise AgentControlError(
                    f"scenario step {index} wait_seconds must be within [0, 30]"
                )
    return commands


def drive_scenario(
    port: int,
    commands: Sequence[dict[str, Any]],
    screenshot_path: str | Path | None = None,
    max_seconds: float = 60.0,
) -> dict[str, Any]:
    """Runs an agent-authored scenario against one running platformer.

    Steps are executed in order after the render-loop warmup. Supported step
    shapes (keys may combine a protocol command with one control directive):

    - protocol commands: ``cmd`` of ``ping``/``state``/``diagnostics``/
      ``inject_key``/``inject_pad_button``/``inject_pad_stick``/``inject_mouse_button``/
      ``inject_mouse_move``/``inject_scroll``/``screenshot``/``game``
    - ``wait_seconds``: bounded sleep
    - ``poll_state``: object of required key/value matches in game state,
      polled until satisfied (``poll_timeout_s`` bounds each poll)
    - ``screenshot``: absolute path capture

    Keys held down via ``inject_key``/``inject_pad_button`` with
    ``down=true`` are released best-effort in the ``finally`` block, and the
    whole run is bounded by ``max_seconds``.
    """
    validate_scenario(list(commands))
    deadline = time.monotonic() + max(max_seconds, 1.0)
    steps: list[dict[str, Any]] = []
    client = AgentClient()
    held_keys: set[str] = set()
    end_state: dict[str, Any] = {}

    def remaining() -> float:
        left = deadline - time.monotonic()
        if left <= 0:
            raise AgentControlError(f"scenario exceeded its {max_seconds:.1f}s overall budget")
        return left

    def issue(command: dict[str, Any]) -> dict[str, Any]:
        payload = {"id": next_request_id(client._used_ids), **command}
        response = client.call(payload, timeout_s=remaining())
        add_step(steps, str(payload.get("cmd", "request")), request=payload, response=response)
        if not response.get("ok"):
            raise AgentControlError(
                f"agent command '{payload.get('cmd')}' failed: {response.get('error', 'unknown error')}"
            )
        return response

    try:
        client.wait_for_port(port, timeout_s=remaining())
        add_step(steps, "connected", request={"port": port})
        for _ in range(120):
            ping = issue({"cmd": "ping"})
            if (ping.get("result") or {}).get("frame", 0) > 30:
                add_step(steps, "render loop running", request={"frame": (ping.get("result") or {}).get("frame")})
                break
            time.sleep(min(0.1, remaining()))
        else:
            raise AgentControlError("game render loop never started (frame stuck near 0)")

        for index, step in enumerate(commands):
            control = {k: step[k] for k in ("wait_seconds", "poll_state", "poll_timeout_s") if k in step}
            protocol = {k: v for k, v in step.items() if k not in control}

            if "wait_seconds" in control:
                time.sleep(min(float(control["wait_seconds"]), remaining()))
                add_step(steps, "waited", request={"seconds": control["wait_seconds"]})

            if protocol:
                if protocol.get("cmd") == "inject_key" and protocol.get("down"):
                    held_keys.add(str(protocol.get("key")))
                elif protocol.get("cmd") == "inject_key" and not protocol.get("down", True):
                    held_keys.discard(str(protocol.get("key")))
                response = issue(protocol)
                if protocol.get("cmd") == "screenshot" and screenshot_path:
                    add_step(steps, "screenshot captured", request={"path": str(screenshot_path)})

            if "poll_state" in control:
                required = control["poll_state"]
                timeout = float(control.get("poll_timeout_s", 10.0))
                poll_deadline = time.monotonic() + min(timeout, remaining())

                def matches(state: dict[str, Any]) -> bool:
                    return all(state.get(key) == value for key, value in required.items())

                while True:
                    state = issue({"cmd": "state"}).get("result") or {}
                    end_state = state
                    if matches(state):
                        add_step(steps, "poll satisfied", request=required, response={"matched": True})
                        break
                    if time.monotonic() >= poll_deadline:
                        raise AgentControlError(
                            f"scenario step {index} poll unsatisfied within {timeout:.1f}s: {required}"
                        )
                    time.sleep(min(POLL_INTERVAL_S, remaining()))

        final = issue({"cmd": "state"}).get("result") or {}
        end_state = final or end_state
        return {
            "steps": steps,
            "command_count": len(commands),
            "final_state": end_state,
            "ok": True,
        }
    finally:
        for key in list(held_keys):
            try:
                client.send({"cmd": "inject_key", "key": key, "down": False})
            except AgentControlError:
                pass
        client.close()
