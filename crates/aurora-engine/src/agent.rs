//! Agent runtime control plane: a small command protocol shared by the
//! native loopback TCP server and the browser JS bridge.
//!
//! An agent (coding assistant, test harness, or human tooling) can inject
//! synthetic input, request screenshots, read game-published state, and call
//! game-specific actions — the same surface a human player has, exposed as
//! newline-delimited JSON. The protocol is deliberately tiny and dependency
//! free: every command is one JSON object with an `id` and a `cmd`.
//!
//! Security posture: native transport binds `127.0.0.1` only and is opt-in
//! via `AURORA_AGENT_PORT`; the web bridge is bound to one page's `window`.
//! Both are development/CI affordances, not a production attack surface.

use serde::Deserialize;
use serde_json::Value;
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

use crate::input::{PadButton, PadStick};

/// Maximum UTF-8 JSON payload (excluding its newline delimiter) accepted by
/// the development agent transport.
pub const MAX_AGENT_FRAME_BYTES: usize = 64 * 1024;
/// Maximum number of valid requests the native server executes in one render
/// frame, keeping a local client from monopolizing the game loop.
pub const MAX_AGENT_REQUESTS_PER_POLL: usize = 64;

const MAX_AGENT_COMMAND_BYTES: usize = 64;
const MAX_AGENT_ACTION_BYTES: usize = 256;
const MAX_AGENT_PATH_BYTES: usize = 4 * 1024;

/// One agent request, parsed and validated.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentCommand {
    Ping,
    /// Latest game-published state ([`crate::Game::agent_state`]).
    State,
    /// Engine diagnostics (fps, frame times, draw calls).
    Diagnostics,
    InjectKey {
        key: KeyCode,
        down: bool,
    },
    InjectPadButton {
        slot: usize,
        button: PadButton,
        down: bool,
    },
    InjectPadStick {
        slot: usize,
        stick: PadStick,
        x: f32,
        y: f32,
    },
    InjectMouseButton {
        button: MouseButton,
        down: bool,
    },
    InjectMouseMove {
        x: f32,
        y: f32,
    },
    InjectScroll {
        delta: f32,
    },
    /// Capture the next rendered frame to a PNG (native only).
    Screenshot {
        path: String,
    },
    /// Game-defined action ([`crate::Game::on_agent_command`]).
    Game {
        action: String,
        args: Value,
    },
}

impl AgentCommand {
    /// Stable name used in logs and protocol docs.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Ping => "ping",
            Self::State => "state",
            Self::Diagnostics => "diagnostics",
            Self::InjectKey { .. } => "inject_key",
            Self::InjectPadButton { .. } => "inject_pad_button",
            Self::InjectPadStick { .. } => "inject_pad_stick",
            Self::InjectMouseButton { .. } => "inject_mouse_button",
            Self::InjectMouseMove { .. } => "inject_mouse_move",
            Self::InjectScroll { .. } => "inject_scroll",
            Self::Screenshot { .. } => "screenshot",
            Self::Game { .. } => "game",
        }
    }
}

/// A parsed request: protocol id plus command.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentRequest {
    pub id: u64,
    pub command: AgentCommand,
}

/// A reply ready for serialization.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentReply {
    pub id: u64,
    pub result: Result<Value, String>,
}

impl AgentReply {
    pub fn to_line(&self) -> String {
        let payload = match &self.result {
            Ok(value) => format!("{{\"id\":{},\"ok\":true,\"result\":{}}}", self.id, value),
            Err(error) => format!(
                "{{\"id\":{},\"ok\":false,\"error\":{}}}",
                self.id,
                serde_json::to_string(error).unwrap_or_else(|_| "\"error\"".to_owned())
            ),
        };
        format!("{payload}\n")
    }
}

#[derive(Deserialize)]
struct RawRequest {
    id: u64,
    cmd: String,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    down: Option<bool>,
    #[serde(default)]
    slot: Option<usize>,
    #[serde(default)]
    button: Option<String>,
    #[serde(default)]
    stick: Option<String>,
    #[serde(default)]
    x: Option<f32>,
    #[serde(default)]
    y: Option<f32>,
    #[serde(default)]
    delta: Option<f32>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    args: Option<Value>,
}

/// Parses one JSON line into a request. Errors are protocol-level and answer
/// directly so a misbehaving agent always gets feedback.
pub fn parse_line(line: &str) -> Result<AgentRequest, String> {
    if line.len() > MAX_AGENT_FRAME_BYTES {
        return Err(format!(
            "agent frame exceeds maximum of {MAX_AGENT_FRAME_BYTES} bytes"
        ));
    }
    let raw: RawRequest =
        serde_json::from_str(line).map_err(|error| format!("bad request: {error}"))?;
    if raw.cmd.len() > MAX_AGENT_COMMAND_BYTES {
        return Err(format!(
            "agent command exceeds maximum of {MAX_AGENT_COMMAND_BYTES} bytes"
        ));
    }
    if raw
        .action
        .as_ref()
        .is_some_and(|value| value.len() > MAX_AGENT_ACTION_BYTES)
    {
        return Err(format!(
            "agent action exceeds maximum of {MAX_AGENT_ACTION_BYTES} bytes"
        ));
    }
    if raw
        .path
        .as_ref()
        .is_some_and(|value| value.len() > MAX_AGENT_PATH_BYTES)
    {
        return Err(format!(
            "agent path exceeds maximum of {MAX_AGENT_PATH_BYTES} bytes"
        ));
    }
    let command = match raw.cmd.as_str() {
        "ping" => AgentCommand::Ping,
        "state" => AgentCommand::State,
        "diagnostics" => AgentCommand::Diagnostics,
        "inject_key" => AgentCommand::InjectKey {
            key: parse_key_code(raw.key.as_deref().unwrap_or_default())
                .ok_or_else(|| "inject_key needs a known key name".to_owned())?,
            down: raw.down.unwrap_or(true),
        },
        "inject_pad_button" => AgentCommand::InjectPadButton {
            slot: raw.slot.unwrap_or(0).min(crate::input::MAX_GAMEPADS - 1),
            button: parse_pad_button(raw.button.as_deref().unwrap_or_default())
                .ok_or_else(|| "inject_pad_button needs a known button name".to_owned())?,
            down: raw.down.unwrap_or(true),
        },
        "inject_pad_stick" => AgentCommand::InjectPadStick {
            slot: raw.slot.unwrap_or(0).min(crate::input::MAX_GAMEPADS - 1),
            stick: parse_pad_stick(raw.stick.as_deref().unwrap_or_default())
                .ok_or_else(|| "inject_pad_stick needs Left or Right".to_owned())?,
            x: raw.x.unwrap_or_default(),
            y: raw.y.unwrap_or_default(),
        },
        "inject_mouse_button" => AgentCommand::InjectMouseButton {
            button: parse_mouse_button(raw.button.as_deref().unwrap_or_default())
                .ok_or_else(|| "inject_mouse_button needs Left|Right|Middle".to_owned())?,
            down: raw.down.unwrap_or(true),
        },
        "inject_mouse_move" => AgentCommand::InjectMouseMove {
            x: raw.x.unwrap_or_default(),
            y: raw.y.unwrap_or_default(),
        },
        "inject_scroll" => AgentCommand::InjectScroll {
            delta: raw.delta.unwrap_or_default(),
        },
        "screenshot" => AgentCommand::Screenshot {
            path: raw
                .path
                .clone()
                .ok_or_else(|| "screenshot needs a path".to_owned())?,
        },
        "game" => AgentCommand::Game {
            action: raw
                .action
                .clone()
                .ok_or_else(|| "game needs an action".to_owned())?,
            args: raw.args.unwrap_or(Value::Null),
        },
        other => return Err(format!("unknown cmd '{other}'")),
    };
    Ok(AgentRequest {
        id: raw.id,
        command,
    })
}

/// Parses a winit `KeyCode` variant name (`KeyD`, `Space`, `ArrowLeft`, ...).
pub fn parse_key_code(name: &str) -> Option<KeyCode> {
    use KeyCode::*;
    Some(match name {
        "Space" => Space,
        "Enter" => Enter,
        "Escape" => Escape,
        "Tab" => Tab,
        "Backspace" => Backspace,
        "ArrowUp" => ArrowUp,
        "ArrowDown" => ArrowDown,
        "ArrowLeft" => ArrowLeft,
        "ArrowRight" => ArrowRight,
        "ShiftLeft" => ShiftLeft,
        "ShiftRight" => ShiftRight,
        "ControlLeft" => ControlLeft,
        "ControlRight" => ControlRight,
        "SuperLeft" => SuperLeft,
        "SuperRight" => SuperRight,
        "F1" => F1,
        "F2" => F2,
        "F3" => F3,
        "F4" => F4,
        "F5" => F5,
        "F6" => F6,
        "F7" => F7,
        "F8" => F8,
        "F9" => F9,
        "F10" => F10,
        "F11" => F11,
        "F12" => F12,
        "Digit0" => Digit0,
        "Digit1" => Digit1,
        "Digit2" => Digit2,
        "Digit3" => Digit3,
        "Digit4" => Digit4,
        "Digit5" => Digit5,
        "Digit6" => Digit6,
        "Digit7" => Digit7,
        "Digit8" => Digit8,
        "Digit9" => Digit9,
        "KeyA" => KeyA,
        "KeyB" => KeyB,
        "KeyC" => KeyC,
        "KeyD" => KeyD,
        "KeyE" => KeyE,
        "KeyF" => KeyF,
        "KeyG" => KeyG,
        "KeyH" => KeyH,
        "KeyI" => KeyI,
        "KeyJ" => KeyJ,
        "KeyK" => KeyK,
        "KeyL" => KeyL,
        "KeyM" => KeyM,
        "KeyN" => KeyN,
        "KeyO" => KeyO,
        "KeyP" => KeyP,
        "KeyQ" => KeyQ,
        "KeyR" => KeyR,
        "KeyS" => KeyS,
        "KeyT" => KeyT,
        "KeyU" => KeyU,
        "KeyV" => KeyV,
        "KeyW" => KeyW,
        "KeyX" => KeyX,
        "KeyY" => KeyY,
        "KeyZ" => KeyZ,
        _ => return None,
    })
}

/// Parses an engine `PadButton` name (`South`, `DpadLeft`, ...).
pub fn parse_pad_button(name: &str) -> Option<PadButton> {
    Some(match name {
        "South" => PadButton::South,
        "East" => PadButton::East,
        "West" => PadButton::West,
        "North" => PadButton::North,
        "LeftShoulder" => PadButton::LeftShoulder,
        "RightShoulder" => PadButton::RightShoulder,
        "Back" => PadButton::Back,
        "Start" => PadButton::Start,
        "DpadUp" => PadButton::DpadUp,
        "DpadDown" => PadButton::DpadDown,
        "DpadLeft" => PadButton::DpadLeft,
        "DpadRight" => PadButton::DpadRight,
        _ => return None,
    })
}

/// Parses the logical stick names used by native agent clients and the web
/// bridge. Keeping this separate from button parsing avoids ambiguous input
/// payloads while preserving one protocol across platforms.
pub fn parse_pad_stick(name: &str) -> Option<PadStick> {
    Some(match name {
        "Left" => PadStick::Left,
        "Right" => PadStick::Right,
        _ => return None,
    })
}

/// Parses a winit `MouseButton` name (`Left`, `Right`, `Middle`).
pub fn parse_mouse_button(name: &str) -> Option<MouseButton> {
    Some(match name {
        "Left" => MouseButton::Left,
        "Right" => MouseButton::Right,
        "Middle" => MouseButton::Middle,
        _ => return None,
    })
}

/// Loopback JSON-lines server handing a running game to an agent.
///
/// One client at a time; reads are drained once per frame from the app loop
/// so injected input lands exactly like real device input. Bind via
/// [`AgentServer::from_env`] (reads `AURORA_AGENT_PORT`).
#[cfg(not(target_arch = "wasm32"))]
pub struct AgentServer {
    listener: std::net::TcpListener,
    stream: Option<std::net::TcpStream>,
    buffer: Vec<u8>,
}

#[cfg(not(target_arch = "wasm32"))]
impl AgentServer {
    /// Binds a loopback-only listener on `port`.
    pub fn bind(port: u16) -> std::io::Result<Self> {
        let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            stream: None,
            buffer: Vec::with_capacity(4096),
        })
    }

    /// Opt-in constructor: `Some(server)` only when `AURORA_AGENT_PORT` is
    /// set to a parseable port. Any bind failure degrades to `None` with a
    /// log line — agent tooling must never take the game down.
    pub fn from_env() -> Option<Self> {
        let port: u16 = std::env::var("AURORA_AGENT_PORT")
            .ok()
            .and_then(|value| value.parse().ok())?;
        match Self::bind(port) {
            Ok(server) => {
                log::info!("agent control server on 127.0.0.1:{port}");
                Some(server)
            }
            Err(error) => {
                log::warn!("AURORA_AGENT_PORT={port} unavailable: {error}");
                None
            }
        }
    }

    pub fn local_port(&self) -> u16 {
        self.listener
            .local_addr()
            .map(|addr| addr.port())
            .unwrap_or_default()
    }

    /// Accepts a pending client (or keeps the current one), drains every
    /// complete request line, and answers protocol-level failures inline.
    /// Returns parsed requests for the caller to execute.
    pub fn poll(&mut self) -> Vec<AgentRequest> {
        if self.stream.is_none() {
            if let Ok((stream, _addr)) = self.listener.accept() {
                let _ = stream.set_nonblocking(true);
                log::info!(
                    "agent client connected: {}",
                    stream
                        .peer_addr()
                        .map_or_else(|_| "unknown".to_owned(), |a| a.to_string())
                );
                self.stream = Some(stream);
                self.buffer.clear();
            }
        }
        let mut stream = match self.stream.take() {
            Some(stream) => stream,
            None => return Vec::new(),
        };

        let mut requests = Vec::new();
        let mut inline_errors: Vec<(u64, String)> = Vec::new();
        let mut chunk = [0_u8; 8192];
        let mut disconnected = false;
        loop {
            match std::io::Read::read(&mut stream, &mut chunk) {
                Ok(0) => {
                    disconnected = true;
                    break;
                }
                Ok(count) => {
                    self.buffer.extend_from_slice(&chunk[..count]);
                    if self.buffer.len() > MAX_AGENT_FRAME_BYTES {
                        log::warn!(
                            "agent client exceeded the {MAX_AGENT_FRAME_BYTES}-byte frame limit"
                        );
                        disconnected = true;
                        break;
                    }
                    while let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
                        if position > MAX_AGENT_FRAME_BYTES {
                            log::warn!(
                                "agent client sent a frame over the {MAX_AGENT_FRAME_BYTES}-byte limit"
                            );
                            disconnected = true;
                            break;
                        }
                        let line: Vec<u8> = self.buffer.drain(..=position).collect();
                        let text = String::from_utf8_lossy(&line[..line.len() - 1]);
                        let text = text.trim();
                        if text.is_empty() {
                            continue;
                        }
                        if requests.len() >= MAX_AGENT_REQUESTS_PER_POLL {
                            let id = request_id(text);
                            inline_errors.push((
                                id,
                                format!("agent request batch exceeds maximum of {MAX_AGENT_REQUESTS_PER_POLL} requests"),
                            ));
                            continue;
                        }
                        match parse_line(text) {
                            Ok(request) => requests.push(request),
                            Err(error) => {
                                let id = request_id(text);
                                inline_errors.push((id, error));
                            }
                        }
                    }
                    if disconnected {
                        break;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    disconnected = true;
                    break;
                }
            }
        }

        if !disconnected {
            self.stream = Some(stream);
        } else {
            self.buffer.clear();
        }
        for (id, error) in inline_errors {
            self.respond(id, Err(error));
        }
        requests
    }

    /// Sends one reply immediately.
    pub fn respond(&mut self, id: u64, result: Result<serde_json::Value, String>) {
        let reply = AgentReply { id, result };
        if let Some(stream) = self.stream.as_mut() {
            let _ = std::io::Write::write_all(stream, reply.to_line().as_bytes());
            let _ = std::io::Write::flush(stream);
        }
    }

    /// True when at least one request arrived since the last poll.
    pub fn has_client(&self) -> bool {
        self.stream.is_some()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn request_id(text: &str) -> u64 {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| value.get("id").and_then(Value::as_u64))
        .unwrap_or(0)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod server_tests {
    use super::*;
    use std::io::{BufRead as _, Write as _};
    use std::time::{Duration, Instant};

    /// Loopback delivery is asynchronous; bounded retry keeps tests honest
    /// without sleeping on happy paths.
    fn wait_for_requests(
        server: &mut AgentServer,
        count: usize,
        timeout: Duration,
    ) -> Vec<AgentRequest> {
        let deadline = Instant::now() + timeout;
        let mut all = Vec::new();
        while all.len() < count && Instant::now() < deadline {
            all.extend(server.poll());
            if all.len() < count {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        all
    }

    #[test]
    fn server_round_trips_requests_over_real_sockets() {
        let mut server = AgentServer::bind(0).expect("ephemeral bind");
        let port = server.local_port();
        assert!(port > 0);

        let mut client =
            std::net::TcpStream::connect(("127.0.0.1", port)).expect("loopback connect");
        client
            .write_all(b"{\"id\":1,\"cmd\":\"ping\"}\n{\"id\":2,\"cmd\":\"nope\"}\n")
            .expect("write requests");

        let requests = wait_for_requests(&mut server, 1, Duration::from_secs(2));
        assert_eq!(requests.len(), 1, "valid request parsed, invalid answered");
        assert_eq!(requests[0].command, AgentCommand::Ping);

        server.respond(requests[0].id, Ok(serde_json::json!({"pong": true})));

        let mut reader = std::io::BufReader::new(&client);
        let mut reply = String::new();
        reader.read_line(&mut reply).expect("read replies");
        assert_eq!(
            reply.trim(),
            "{\"id\":2,\"ok\":false,\"error\":\"unknown cmd 'nope'\"}",
            "inline protocol errors are answered first"
        );
        reply.clear();
        reader.read_line(&mut reply).expect("read replies");
        assert_eq!(
            reply.trim(),
            "{\"id\":1,\"ok\":true,\"result\":{\"pong\":true}}"
        );

        let second = server.poll();
        assert!(second.is_empty(), "invalid request answered inline");
    }

    #[test]
    fn split_lines_reassemble_across_polls() {
        let mut server = AgentServer::bind(0).expect("ephemeral bind");
        let port = server.local_port();
        let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
        client.write_all(b"{\"id\":").expect("half a line");

        assert!(server.poll().is_empty(), "partial line is not a request");

        client
            .write_all(b"5,\"cmd\":\"state\"}\n")
            .expect("rest of line");
        let requests = wait_for_requests(&mut server, 1, Duration::from_secs(2));
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].id, 5);
        assert_eq!(requests[0].command, AgentCommand::State);
    }

    #[test]
    fn request_batches_are_capped_per_poll() {
        let mut server = AgentServer::bind(0).expect("ephemeral bind");
        let port = server.local_port();
        let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let request_count = MAX_AGENT_REQUESTS_PER_POLL + 8;
        let payload = (0..request_count)
            .map(|id| format!("{{\"id\":{id},\"cmd\":\"ping\"}}\n"))
            .collect::<String>();
        client
            .write_all(payload.as_bytes())
            .expect("write requests");

        let requests = wait_for_requests(
            &mut server,
            MAX_AGENT_REQUESTS_PER_POLL,
            Duration::from_secs(2),
        );
        assert_eq!(requests.len(), MAX_AGENT_REQUESTS_PER_POLL);
    }

    #[test]
    fn disconnects_are_tolerated_and_slots_reused() {
        let mut server = AgentServer::bind(0).expect("ephemeral bind");
        let port = server.local_port();
        let client = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
        drop(client);

        let _ = server.poll();
        let reconnect = std::net::TcpStream::connect(("127.0.0.1", port));
        assert!(
            reconnect.is_ok(),
            "server accepts a new client after disconnect"
        );
        let _ = server.poll();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_covers_every_engine_command() {
        let request = parse_line(r#"{"id":7,"cmd":"inject_key","key":"KeyD","down":true}"#)
            .expect("valid inject_key");
        assert_eq!(request.id, 7);
        assert_eq!(
            request.command,
            AgentCommand::InjectKey {
                key: KeyCode::KeyD,
                down: true
            }
        );

        let request = parse_line(
            r#"{"id":1,"cmd":"inject_pad_button","button":"South","down":true,"slot":2}"#,
        )
        .expect("valid pad inject");
        assert_eq!(
            request.command,
            AgentCommand::InjectPadButton {
                slot: 2,
                button: PadButton::South,
                down: true
            }
        );

        let request = parse_line(
            r#"{"id":2,"cmd":"inject_pad_stick","stick":"Right","x":0.75,"y":-0.25,"slot":1}"#,
        )
        .expect("valid pad stick inject");
        assert_eq!(
            request.command,
            AgentCommand::InjectPadStick {
                slot: 1,
                stick: PadStick::Right,
                x: 0.75,
                y: -0.25,
            }
        );

        let request =
            parse_line(r#"{"id":9,"cmd":"screenshot","path":"/tmp/frame.png"}"#).expect("shot");
        assert_eq!(
            request.command,
            AgentCommand::Screenshot {
                path: "/tmp/frame.png".to_owned()
            }
        );

        let request =
            parse_line(r#"{"id":3,"cmd":"game","action":"teleport","args":{"x":10,"y":5}}"#)
                .expect("game");
        assert_eq!(
            request.command,
            AgentCommand::Game {
                action: "teleport".to_owned(),
                args: serde_json::json!({"x": 10, "y": 5})
            }
        );

        for cmd in ["ping", "state", "diagnostics"] {
            let request = parse_line(&format!(r#"{{"id":1,"cmd":"{cmd}"}}"#)).expect(cmd);
            assert_eq!(request.command.name(), cmd);
        }
    }

    #[test]
    fn malformed_requests_report_not_panic() {
        assert!(parse_line("").is_err());
        assert!(parse_line("{ not json").is_err());
        assert!(parse_line(r#"{"id":1}"#).is_err(), "missing cmd");
        assert!(
            parse_line(r#"{"id":1,"cmd":"warp"}"#).is_err(),
            "unknown cmd"
        );
        assert!(
            parse_line(r#"{"id":1,"cmd":"inject_key"}"#).is_err(),
            "missing key"
        );
        assert!(
            parse_line(r#"{"id":1,"cmd":"inject_key","key":"KeyQ11"}"#).is_err(),
            "unknown key name"
        );
        assert!(parse_line(r#"{"id":1,"cmd":"inject_mouse_button","button":"Side"}"#).is_err());
        assert!(parse_line(r#"{"id":1,"cmd":"inject_pad_stick","stick":"Center"}"#).is_err());
    }

    #[test]
    fn oversized_frames_are_rejected_before_json_parsing() {
        let line = format!(
            r#"{{"id":1,"cmd":"game","action":"{}"}}"#,
            "x".repeat(MAX_AGENT_FRAME_BYTES)
        );
        let error = parse_line(&line).expect_err("oversized frame must be rejected");
        assert!(error.contains("maximum"), "unexpected error: {error}");
    }

    #[test]
    fn replies_serialize_to_single_lines() {
        let ok = AgentReply {
            id: 4,
            result: Ok(serde_json::json!({"moved": true})),
        };
        assert_eq!(
            ok.to_line(),
            "{\"id\":4,\"ok\":true,\"result\":{\"moved\":true}}\n"
        );

        let error = AgentReply {
            id: 5,
            result: Err("nope".to_owned()),
        };
        assert_eq!(
            error.to_line(),
            "{\"id\":5,\"ok\":false,\"error\":\"nope\"}\n"
        );
    }
}
