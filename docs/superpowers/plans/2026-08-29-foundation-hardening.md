# Foundation Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make edge-triggered input, WASM diagnostics, agent transport, and local validation deterministic and bounded.

**Architecture:** The app shell labels each fixed-step callback so existing `Input` queries expose edge state only on fixed step zero. The native agent remains a loopback JSON-lines server, but all frame, batch, and text inputs receive explicit caps. Validation scripts build both web artifacts and run strict WASM clippy before browser tests.

**Tech Stack:** Rust, winit 0.30, serde/serde_json, Cargo, Trunk, Playwright, Bash.

**Spec:** `docs/superpowers/specs/2026-08-29-aurora-evolution-design.md`

## Global Constraints

- Preserve the additive `FrameCtx` and `Input` API shape; fixed-step edge gating is internal to the app/input shell.
- Native and `wasm32-unknown-unknown` builds must remain supported.
- No new Rust dependencies.
- Oversized agent input must not panic or stall the render loop.
- Validation commands must fail closed on required lanes.

---

### Task 1: Prove fixed-step edge semantics and gamepad accumulation

**Files:**
- Modify: `crates/aurora-engine/src/input.rs:130-190,300-360,560-610`
- Test: `crates/aurora-engine/src/input.rs:607-780`

**Interfaces:**
- Produces `pub(crate) fn begin_fixed_step(&mut self, step: usize)` and `pub(crate) fn end_fixed_steps(&mut self)`.
- Existing `key_pressed`, `key_released`, `mouse_pressed`, `mouse_released`, and `pad_button_pressed` remain callable through shared references.

- [ ] **Step 1: Write the failing tests**

Add tests named `fixed_step_edges_are_visible_once_and_return_for_frame_update` and `synthetic_pad_press_survives_same_frame_release`. The first injects `KeyP`, calls `begin_fixed_step(0)`, `begin_fixed_step(1)`, `end_fixed_steps`, and `begin_frame`, asserting true, false, true, false. The second presses and releases `PadButton::East` before `begin_frame` and asserts the button is no longer down but remains pressed.

- [ ] **Step 2: Run the focused tests to verify they fail**

Run: `cargo test -p aurora-engine input::tests::fixed_step_edges_are_visible_once_and_return_for_frame_update input::tests::synthetic_pad_press_survives_same_frame_release -- --exact`

Expected: compilation or test failure because the fixed-step methods do not exist and `apply_buttons` clears the earlier edge.

- [ ] **Step 3: Implement the minimal input phase state**

Add a private `fixed_step: Option<usize>` field defaulting to `None`. `begin_fixed_step(step)` stores `Some(step)`, `end_fixed_steps()` clears it, and a private `edge_visible()` returns true outside fixed callbacks or for step zero. Gate all edge-query methods with `edge_visible()`. Change `apply_buttons` to OR newly detected presses into `buttons_pressed` instead of resetting the array on every synthetic update.

- [ ] **Step 4: Run the focused tests to verify they pass**

Run: `cargo test -p aurora-engine input::tests::fixed_step_edges_are_visible_once_and_return_for_frame_update input::tests::synthetic_pad_press_survives_same_frame_release -- --exact`

Expected: both tests pass.

- [ ] **Step 5: Run the full engine input tests**

Run: `cargo test -p aurora-engine input::tests`

Expected: all input tests pass with no warnings.

### Task 2: Wire the app fixed-step phase and remove duplicate Last Light gating

**Files:**
- Modify: `crates/aurora-engine/src/app.rs:120-220,735-810`
- Modify: `games/last-light/src/main.rs:125-135,330-350,510-525,5810-5970,10510-10525`
- Test: `demos/platformer/src/main.rs` browser contract at `playtests/browser/agent-control.spec.mjs:87-120`

**Interfaces:**
- `EngineApp` invokes `Input::begin_fixed_step` before each fixed callback and `Input::end_fixed_steps` before `on_update`.
- Last Light relies on the engine contract rather than a game-local suppression flag.

- [ ] **Step 1: Run the existing browser regression before the wiring change**

Run: `npx playwright test playtests/browser/agent-control.spec.mjs -g "pause, resume, and dash"`

Expected: the test fails at the resume assertion with `Received: "paused"`.

- [ ] **Step 2: Add the app phase transitions**

Call `self.input.begin_fixed_step(fixed_steps)` immediately before constructing the fixed `FrameCtx`, increment the step counter after the callback, and call `self.input.end_fixed_steps()` after the catch-up loop (including the zero-step path). Leave `begin_frame()` at the existing end-of-frame boundary.

- [ ] **Step 3: Remove Last Light's redundant suppression state**

Delete `edge_input_allowed`, the `input_handled_this_frame` field/init/reset, and use the existing edge queries directly in `on_fixed_update`. Keep continuous simulation outside the edge branches unchanged.

- [ ] **Step 4: Rebuild and run the focused browser test**

Run: `env -u NO_COLOR ./scripts/build-platformer-web.sh && npx playwright test playtests/browser/agent-control.spec.mjs -g "pause, resume, and dash"`

Expected: the pause/resume/dash test passes.

- [ ] **Step 5: Run native regression coverage**

Run: `cargo test --workspace`

Expected: all existing tests pass.

### Task 3: Bound the native agent protocol

**Files:**
- Modify: `crates/aurora-engine/src/agent.rs:100-180,285-390`
- Test: `crates/aurora-engine/src/agent.rs:500-620`
- Modify: `tools/aurora-mcp/agent_control.py:150-220`
- Test: `tools/aurora-mcp/test_protocol.py`

**Interfaces:**
- Produces `MAX_AGENT_FRAME_BYTES`, `MAX_AGENT_REQUESTS_PER_POLL`, and bounded parser errors.
- `AgentClient.send` rejects an encoded frame larger than the Rust frame cap with `AgentControlError`.

- [ ] **Step 1: Write failing Rust and Python boundary tests**

Add Rust tests that reject a JSON line longer than the frame limit and verify a press/release flood cannot return more than the per-poll request cap. Add a Python unit test that `AgentClient.send` rejects an oversized JSON payload before writing.

- [ ] **Step 2: Run the boundary tests to verify they fail**

Run: `cargo test -p aurora-engine agent::tests -- --nocapture` and `python3 tools/aurora-mcp/test_protocol.py`

Expected: the new size/cap assertions fail because the current parser and client are unbounded.

- [ ] **Step 3: Implement bounded parsing and polling**

Reject frames over 64 KiB in `parse_line`. In `AgentServer::poll`, disconnect and clear the current client when the partial buffer crosses the same cap, reject an oversized complete line before draining it, and stop accepting requests after 64 valid requests in one poll. Keep inline protocol errors for bounded malformed frames. Cap Python outgoing frames at 64 KiB.

- [ ] **Step 4: Run protocol tests to verify they pass**

Run: `cargo test -p aurora-engine agent::tests server_tests -- --nocapture` and `python3 tools/aurora-mcp/test_protocol.py`

Expected: Rust socket tests and Python protocol tests pass.

### Task 4: Clean WASM and make validation execute the real release lanes

**Files:**
- Modify: `crates/aurora-engine/src/web_agent.rs:18-35`
- Modify: `crates/aurora-engine/src/save.rs:260-280`
- Modify: `crates/aurora-engine/src/renderer.rs:280-410,430-670`
- Create: `scripts/build-platformer-web.sh`
- Modify: `scripts/validate-local.sh:1-25`

**Interfaces:**
- `scripts/build-platformer-web.sh` builds `demos/platformer` with `env -u NO_COLOR trunk build --release`.
- Local validation runs native clippy, strict WASM clippy, both web builds, browser tests, tests, and budgets.

- [ ] **Step 1: Run strict WASM clippy to capture the current failures**

Run: `cargo clippy --workspace --target wasm32-unknown-unknown --all-targets -- -D warnings`

Expected: current duplicate cfg, save test, unused native/3D declarations, and thread-local initializer errors are listed.

- [ ] **Step 2: Apply cfg and initializer fixes**

Remove the inner `#![cfg(target_arch = "wasm32")]` from `web_agent.rs`, gate save tests with `#[cfg(all(test, not(target_arch = "wasm32")))]`, use `const { RefCell::new(Value::Null) }`, and add precise WASM cfg/allow annotations for renderer code that is intentionally native/3D-only.

- [ ] **Step 3: Add the platformer build wrapper**

Create the executable script with the same root discovery and target installation behavior as `scripts/build-web.sh`, then run `env -u NO_COLOR trunk build --release` from `demos/platformer`.

- [ ] **Step 4: Extend the validation gate**

Run strict WASM clippy after WASM check, call both build wrappers, run `npm run test:browser`, and run both budget scripts after builds. Preserve the existing trace test and fail if required tools are missing.

- [ ] **Step 5: Verify the complete Track A gate**

Run: `./scripts/validate-local.sh`

Expected: exit 0 with native/WASM clippy, tests, both Trunk builds, browser tests, and budgets all passing.

