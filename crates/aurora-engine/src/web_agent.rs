//! Browser agent bridge (WASM only): exposes the same agent control plane as
//! the native loopback server, bound to the page's `window` object instead of
//! a TCP socket.
//!
//! With the bridge installed (it installs automatically on web), an agent —
//! a Playwright harness, a bookmarklet, or an MCP-web client — can drive the
//! game from page JavaScript:
//!
//! ```js
//! window.auroraInjectKey("Space", true);   // synthetic keyboard
//! window.auroraInjectKey("Space", false);
//! window.auroraInjectPad("South", true);   // synthetic gamepad (slot 0)
//! window.auroraInjectPad("South", false);
//! window.auroraInjectPadStick("Right", 1, 0); // virtual cursor input
//! window.auroraInjectMouseMove(640, 360);   // logical/CSS coordinates
//! window.auroraInjectMouseButton("Left", true);
//! window.auroraInjectMouseButton("Left", false);
//! window.auroraGame("reset", "{}");         // queued game-specific action
//! const state = JSON.parse(window.auroraState()); // game-published state
//! ```
//!
//! Injected commands are queued and drained at the top of the next frame, so
//! they land with identical semantics to real device input.

use std::cell::RefCell;

use serde_json::Value;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use crate::agent::{
    parse_key_code, parse_mouse_button, parse_pad_button, parse_pad_stick, AgentCommand,
};
use crate::input::Input;

thread_local! {
    static QUEUE: RefCell<Vec<AgentCommand>> = const { RefCell::new(Vec::new()) };
    static STATE: RefCell<Value> = const { RefCell::new(Value::Null) };
}

/// Called from the app loop before game logic: applies queued injections.
pub fn drain(input: &mut Input) -> Vec<(String, Value)> {
    let commands = QUEUE.with(|queue| std::mem::take(&mut *queue.borrow_mut()));
    let mut game_commands = Vec::new();
    for command in commands {
        match command {
            AgentCommand::InjectKey { key, down } => input.simulate_key(key, down),
            AgentCommand::InjectPadButton { slot, button, down } => {
                input.simulate_gamepad_button(slot, button, down)
            }
            AgentCommand::InjectPadStick { slot, stick, x, y } => {
                input.simulate_gamepad_stick(slot, stick, glam::Vec2::new(x, y))
            }
            AgentCommand::InjectMouseButton { button, down } => {
                input.simulate_mouse_button(button, down)
            }
            AgentCommand::InjectMouseMove { x, y } => {
                input.simulate_mouse_position(glam::Vec2::new(x, y))
            }
            AgentCommand::InjectScroll { delta } => input.simulate_scroll(delta),
            AgentCommand::Game { action, args } => game_commands.push((action, args)),
            // Non-injection commands have no queue-side meaning on web;
            // state/diagnostics are published every frame.
            _ => {}
        }
    }
    game_commands
}

/// Called from the app loop after updates: publishes fresh game state.
pub fn publish(state: Option<Value>) {
    STATE.with(|slot| *slot.borrow_mut() = state.unwrap_or(Value::Null));
}

fn queue_command(command: AgentCommand) {
    QUEUE.with(|queue| queue.borrow_mut().push(command));
}

fn report_error(context: &str, error: &JsValue) {
    log::warn!("web agent bridge {context}: {error:?}");
}

/// Installs `window.aurora*` bridge functions. Called once during engine
/// startup on web; safe to call again (re-registers the same closures).
pub fn install() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let target: JsValue = window.into();

    macro_rules! bridge {
        ($name:literal, $closure:expr) => {{
            let function = $closure;
            if let Err(error) = js_sys::Reflect::set(
                &target,
                &JsValue::from_str($name),
                function.as_ref().unchecked_ref(),
            ) {
                report_error($name, &error);
            }
            function.forget();
        }};
    }

    let inject_key = Closure::wrap(Box::new(move |code: String, down: bool| {
        if let Some(key) = parse_key_code(&code) {
            queue_command(AgentCommand::InjectKey { key, down });
        } else {
            log::warn!("auroraInjectKey: unknown key '{code}'");
        }
    }) as Box<dyn FnMut(String, bool)>);
    bridge!("auroraInjectKey", inject_key);

    let inject_pad = Closure::wrap(Box::new(move |button: String, down: bool| {
        if let Some(pad_button) = parse_pad_button(&button) {
            queue_command(AgentCommand::InjectPadButton {
                slot: 0,
                button: pad_button,
                down,
            });
        } else {
            log::warn!("auroraInjectPad: unknown button '{button}'");
        }
    }) as Box<dyn FnMut(String, bool)>);
    bridge!("auroraInjectPad", inject_pad);

    let inject_pad_stick = Closure::wrap(Box::new(move |stick: String, x: f32, y: f32| {
        if let Some(pad_stick) = parse_pad_stick(&stick) {
            queue_command(AgentCommand::InjectPadStick {
                slot: 0,
                stick: pad_stick,
                x,
                y,
            });
        } else {
            log::warn!("auroraInjectPadStick: unknown stick '{stick}'");
        }
    }) as Box<dyn FnMut(String, f32, f32)>);
    bridge!("auroraInjectPadStick", inject_pad_stick);

    let inject_mouse_move = Closure::wrap(Box::new(move |x: f32, y: f32| {
        queue_command(AgentCommand::InjectMouseMove { x, y });
    }) as Box<dyn FnMut(f32, f32)>);
    bridge!("auroraInjectMouseMove", inject_mouse_move);

    let inject_mouse_button = Closure::wrap(Box::new(move |button: String, down: bool| {
        if let Some(button) = parse_mouse_button(&button) {
            queue_command(AgentCommand::InjectMouseButton { button, down });
        } else {
            log::warn!("auroraInjectMouseButton: unknown button '{button}'");
        }
    }) as Box<dyn FnMut(String, bool)>);
    bridge!("auroraInjectMouseButton", inject_mouse_button);

    let inject_scroll = Closure::wrap(Box::new(move |delta: f32| {
        queue_command(AgentCommand::InjectScroll { delta });
    }) as Box<dyn FnMut(f32)>);
    bridge!("auroraInjectScroll", inject_scroll);

    let game =
        Closure::wrap(Box::new(
            move |action: String, args_json: String| match serde_json::from_str(&args_json) {
                Ok(args) => queue_command(AgentCommand::Game { action, args }),
                Err(error) => log::warn!("auroraGame: invalid args JSON: {error}"),
            },
        ) as Box<dyn FnMut(String, String)>);
    bridge!("auroraGame", game);

    let state = Closure::wrap(Box::new(move || {
        STATE.with(|slot| {
            serde_json::to_string(&*slot.borrow()).unwrap_or_else(|_| "null".to_owned())
        })
    }) as Box<dyn FnMut() -> String>);
    bridge!("auroraState", state);

    log::info!("web agent bridge installed on window");
}
