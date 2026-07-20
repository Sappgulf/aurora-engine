# Last Light RTS interaction rules

This note is the compact design contract for the Last Light tactical loop. It
translates the readable, high-leverage conventions of classic real-time
strategy games into Aurora's smaller specialist squad format.

## Information hierarchy

The world owns the screen. The HUD should be a quiet instrument panel, not a
permanent wall of labels:

1. Keep mission progress, the three core resources, and the current comms
   transmission visible at the top edge.
2. Show a selected squad card only when something is selected. A mixed squad
   gets portrait chips; clicking a chip splits the selection without opening a
   modal.
3. Replace the command card with the selected role's verbs. Hide unavailable
   verbs and zero-value counters until the player can act on them.
4. Use a short-lived toast for a state change (relay restored, node dry,
   production ready), then let it disappear so the player can read the map.

## Selection and command semantics

Aurora follows the classic RTS distinction between intent and execution:

- **Move** means travel to the point and do not acquire a new target on the
  way. It is the safe order for repositioning a specialist.
- **Attack-move** means travel while acquiring visible hostile targets. It is
  the squad's front-line order and should use a distinct cursor/telegraph.
- **Stop** cancels movement, attack, and queued work. **Hold** cancels travel
  while retaining local defensive fire.
- Shift queues an order. Control groups recall a squad. Space centers the
  camera on the latest transmission. These rules make the keyboard useful
  without making the HUD louder.

## Economy readability

Salvage is the tactical spend resource; Flux is the slower power budget. A
Surveyor carries a finite load from a node back to the Lantern, while an
Engineer restores relays and repairs structures. A node should communicate
its full lifecycle visually: available, worker count, carrying, returning,
and dry. Keep the numeric readout in the selected-role card and reserve the
world label for the state that needs attention.

The opening minute is intentionally a teaching window: the first raid waits
long enough for the player to deploy, power a relay, and queue one specialist.
Subsequent raids tighten the pressure only after the economy can support a
response.

## Terrain and counter-play

Terrain is a decision, not decoration. Cyan ridges grant sight and a firing
position; violet pockets reduce incoming pressure. The minimap uses the same
colors, but the legend appears only during onboarding or when the player
pauses over a terrain affordance. Units should never be hidden behind a
terrain label while fighting.

## Role contract

Each named specialist needs one job that is valuable before combat, one job
that is valuable during combat, and one reason to keep them alive:

| Role | Economy / map job | Combat job | Signature decision |
| --- | --- | --- | --- |
| Warden | Hold a powered relay | Anchor and focus-fire | Spend time holding or push the line |
| Engineer | Restore relays and repair | Emergency repair / beacon | Repair now or build future supply |
| Surveyor | Harvest finite salvage and scan | Reveal a safe route | Keep the carrier safe or greed the node |

The command card must name the verb and its cost/constraint. The world should
answer with a small, reversible signal: a path, a progress ring, a repair
beam, or a dry-node pulse.

## Reference patterns

These rules are grounded in the original StarCraft interaction model rather
than copied art or faction fiction: [Hot Keys and Special
Commands](https://classic.battle.net/scc/GS/control.shtml), [Resources](https://classic.battle.net/scc/gs/res.shtml), [High Ground and
Cover](https://classic.battle.net/scc/gs/hc.shtml), [Unit
Commands](https://classic.battle.net/scc/GS/com.shtml), and [Damage Types and
Unit Sizes](https://classic.battle.net/scc/GS/damage.shtml).

### Research pass: economy, scouting, and production pressure

Blizzard's current strategy guides describe the same high-value loop in
plain terms: worker travel distance changes income, scouting spends one unit's
attention to reveal the opponent, and unspent resources are a signal to add
production capacity. The relevant guides are [Resources](https://news.blizzard.com/en-us/article/4488900/game-guide-resources),
[Scouting](https://news.blizzard.com/en-us/article/4488316/game-guide-scouting),
[Economy](https://news.blizzard.com/en-us/article/4488313/game-guide-economy), and
[Buildings](https://news.blizzard.com/en-us/article/4488317/game-guide-buildings).

Aurora's translation is deliberately smaller and deterministic:

- Surveyor haul distance and finite node saturation make route length and
  worker assignment visible decisions, not passive income counters.
- Fog, scan pulses, raid forecasts, and the minimap let a Surveyor buy
  information while the Warden and Engineer keep the line alive.
- Fabricator queues, supply modules, rally points, and contextual build rows
  keep Salvage/Flux spendable without requiring a permanent production panel.
- Every new map should offer one safe opening pocket, one contested resource,
  and one information advantage so economy, scouting, and combat overlap in a
  readable midgame window.
