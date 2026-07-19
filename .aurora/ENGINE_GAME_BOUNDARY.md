# Engine and Game Boundary

## Aurora owns

- deterministic ticks, semantic trace transport, state-hash primitives, and replay reports
- rendering, input devices and configurable platform policy
- generic RTS storage, orders, navigation, vision, economy primitives, and diagnostics
- generic assets, UI layout, audio playback, save envelopes, and bounded devtools

## Games own

- command meaning, factions, units, combat balance, resources, and technology
- campaign progress, relationships, dialogue, missions, objectives, and victory rules
- screen state, HUD content, accessibility wording, audio cues, and asset composition
- state snapshots and migrations

## Proof rule

An engine change needs a second consumer, contract test, or demo. A game change
must not move lore or balance into Aurora merely to make it reusable.

