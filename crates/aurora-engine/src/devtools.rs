//! Native-only debug harness: scripted input + screenshot capture, driven by
//! environment variables. Lets a game be driven and visually inspected
//! without real OS-level input or window-capture access.
//!
//! - `AURORA_SCREENSHOT_PATH` + `AURORA_SCREENSHOT_INTERVAL_MS` (default
//!   500): repeatedly overwrite a PNG with the current frame.
//! - `AURORA_INPUT_SCRIPT`: path to a text file of timestamped synthetic
//!   input events, one per line (blank lines and `#` comments ignored):
//!
//!   ```text
//!   <elapsed_ms> KEY <KeyCodeName> <DOWN|UP>
//!   <elapsed_ms> MOUSE_MOVE <x> <y>
//!   <elapsed_ms> MOUSE_BUTTON <Left|Right|Middle> <DOWN|UP>
//!   <elapsed_ms> SCROLL <delta>
//!   <elapsed_ms> SCREENSHOT <path>
//!   ```
//!
//!   `<KeyCodeName>` is a winit `KeyCode` variant name verbatim (`KeyQ`,
//!   `Space`, `ArrowUp`, `Digit1`, ...).
//!
//! Both are strictly opt-in: with neither environment variable set,
//! [`DebugHarness::from_env`] returns `None` and nothing about the normal
//! game loop changes.

use std::path::PathBuf;

use glam::Vec2;
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

use crate::input::Input;
use crate::renderer::Renderer;

#[derive(Debug, Clone)]
enum ScriptEvent {
    Key(KeyCode, bool),
    MouseMove(Vec2),
    MouseButton(MouseButton, bool),
    Scroll(f32),
    Screenshot(PathBuf),
}

pub struct DebugHarness {
    events: Vec<(f32, ScriptEvent)>,
    cursor: usize,
    periodic_path: Option<PathBuf>,
    periodic_interval_ms: f32,
    since_periodic_ms: f32,
}

impl DebugHarness {
    pub fn from_env() -> Option<Self> {
        let script_path = std::env::var("AURORA_INPUT_SCRIPT").ok();
        let periodic_path = std::env::var("AURORA_SCREENSHOT_PATH")
            .ok()
            .map(PathBuf::from);
        if script_path.is_none() && periodic_path.is_none() {
            return None;
        }
        let events = script_path
            .map(|path| parse_script(&path))
            .unwrap_or_default();
        let periodic_interval_ms = std::env::var("AURORA_SCREENSHOT_INTERVAL_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(500.0);
        log::info!(
            "Aurora devtools: {} scripted events, periodic capture: {}",
            events.len(),
            periodic_path.is_some()
        );
        Some(Self {
            events,
            cursor: 0,
            periodic_path,
            periodic_interval_ms,
            since_periodic_ms: periodic_interval_ms,
        })
    }

    /// Call once per frame with total elapsed and this-frame delta (both
    /// seconds). Applies any due scripted events and arms periodic capture.
    pub fn tick(
        &mut self,
        elapsed_secs: f32,
        dt_secs: f32,
        input: &mut Input,
        renderer: &mut Renderer,
    ) {
        let elapsed_ms = elapsed_secs * 1000.0;
        while self.cursor < self.events.len() && self.events[self.cursor].0 <= elapsed_ms {
            match &self.events[self.cursor].1 {
                ScriptEvent::Key(code, pressed) => input.simulate_key(*code, *pressed),
                ScriptEvent::MouseMove(position) => input.simulate_mouse_position(*position),
                ScriptEvent::MouseButton(button, pressed) => {
                    input.simulate_mouse_button(*button, *pressed)
                }
                ScriptEvent::Scroll(delta) => input.simulate_scroll(*delta),
                ScriptEvent::Screenshot(path) => renderer.request_screenshot(path.clone()),
            }
            self.cursor += 1;
        }

        if let Some(path) = &self.periodic_path {
            self.since_periodic_ms += dt_secs * 1000.0;
            if self.since_periodic_ms >= self.periodic_interval_ms {
                self.since_periodic_ms = 0.0;
                renderer.request_screenshot(path.clone());
            }
        }
    }
}

fn parse_script(path: &str) -> Vec<(f32, ScriptEvent)> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        log::warn!("AURORA_INPUT_SCRIPT: could not read {path}");
        return Vec::new();
    };
    let mut events = Vec::new();
    for (line_number, line) in contents.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parse_line(&parts) {
            Some(event) => events.push(event),
            None => log::warn!(
                "AURORA_INPUT_SCRIPT: skipping malformed line {}",
                line_number + 1
            ),
        }
    }
    events.sort_by(|a, b| a.0.total_cmp(&b.0));
    events
}

fn parse_line(parts: &[&str]) -> Option<(f32, ScriptEvent)> {
    let timestamp: f32 = parts.first()?.parse().ok()?;
    let event = match *parts.get(1)? {
        "KEY" => ScriptEvent::Key(parse_key_code(parts.get(2)?)?, parse_edge(parts.get(3)?)?),
        "MOUSE_MOVE" => ScriptEvent::MouseMove(Vec2::new(
            parts.get(2)?.parse().ok()?,
            parts.get(3)?.parse().ok()?,
        )),
        "MOUSE_BUTTON" => ScriptEvent::MouseButton(
            parse_mouse_button(parts.get(2)?)?,
            parse_edge(parts.get(3)?)?,
        ),
        "SCROLL" => ScriptEvent::Scroll(parts.get(2)?.parse().ok()?),
        "SCREENSHOT" => ScriptEvent::Screenshot(PathBuf::from(*parts.get(2)?)),
        _ => return None,
    };
    Some((timestamp, event))
}

fn parse_edge(value: &str) -> Option<bool> {
    match value {
        "DOWN" => Some(true),
        "UP" => Some(false),
        _ => None,
    }
}

fn parse_mouse_button(value: &str) -> Option<MouseButton> {
    crate::agent::parse_mouse_button(value)
}

/// Polls a file's modification time so games can hot-reload data (levels,
/// tuning, shaders) while running. Deliberately dependency-free: a stat per
/// poll is plenty for authoring loops and works everywhere std does.
#[derive(Debug, Clone)]
pub struct FileWatcher {
    path: PathBuf,
    last_modified: Option<std::time::SystemTime>,
}

impl FileWatcher {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let last_modified = std::fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok();
        Self {
            path,
            last_modified,
        }
    }

    /// Returns `Some(new_mtime)` exactly once per detected change, then
    /// rebaselines. A missing file reports as "changed" only after having
    /// existed (deletions notify so callers can keep the last good data).
    pub fn poll(&mut self) -> Option<std::time::SystemTime> {
        let current = std::fs::metadata(&self.path)
            .and_then(|meta| meta.modified())
            .ok();
        if current != self.last_modified {
            let changed = current;
            self.last_modified = current;
            return changed;
        }
        None
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[cfg(test)]
mod watch_tests {
    use super::*;

    #[test]
    fn watcher_reports_changes_once_then_rebaselines() {
        let path = std::env::temp_dir().join(format!(
            "aurora-watch-test-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::write(&path, b"v1").expect("write fixture");

        let mut watcher = FileWatcher::new(&path);
        assert!(watcher.poll().is_none(), "unchanged file reports nothing");

        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, b"v2-with-different-length").expect("rewrite fixture");
        assert!(watcher.poll().is_some(), "rewrite reports a change");
        assert!(watcher.poll().is_none(), "change fires exactly once");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn missing_files_are_tolerated() {
        let mut watcher = FileWatcher::new("/nonexistent/aurora/watch/target.json");
        assert!(watcher.poll().is_none());
    }
}

fn parse_key_code(name: &str) -> Option<KeyCode> {
    crate::agent::parse_key_code(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_orders_mixed_events_by_timestamp() {
        let parts: Vec<&str> = "500 KEY Space DOWN".split_whitespace().collect();
        assert!(matches!(
            parse_line(&parts),
            Some((500.0, ScriptEvent::Key(KeyCode::Space, true)))
        ));

        let parts: Vec<&str> = "10 MOUSE_MOVE 400 300".split_whitespace().collect();
        assert!(matches!(
            parse_line(&parts),
            Some((10.0, ScriptEvent::MouseMove(position))) if position == Vec2::new(400.0, 300.0)
        ));

        let parts: Vec<&str> = "10 MOUSE_BUTTON Left DOWN".split_whitespace().collect();
        assert!(matches!(
            parse_line(&parts),
            Some((10.0, ScriptEvent::MouseButton(MouseButton::Left, true)))
        ));

        assert!(parse_line(&["bad", "KEY", "Space", "DOWN"]).is_none());
        assert!(parse_line(&["10", "NOPE"]).is_none());
    }
}
