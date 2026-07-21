# Last Light RTS interaction contract

Last Light borrows the proven *interaction shape* of classic real-time
strategy games while keeping its own fiction, roster, and visual language.
This is a product contract for future missions and UI work, not a list of
implementation trivia.

## Player verbs that must stay obvious

- **Select**: click, drag marquee, Shift-add, Ctrl-click a role, and control
  groups 1–5.
- **Move**: right-click ground; Shift queues a waypoint; the camera and
  minimap both remain valid navigation surfaces.
- **Fight**: right-click a visible hostile for a direct attack; `A` arms
  attack-move; `H` holds a firing line; `T` stops the current order.
- **Work**: a Surveyor has an explicit node assignment and a visible
  node → depot → node cargo loop. An Engineer restores relays, repairs allies,
  and deploys field beacons. A Warden anchors a firing line and contests
  high ground.
- **Build**: selecting the Fabricator exposes only production and
  infrastructure actions that can apply in the current power/resource state.

## Information hierarchy

1. The world, silhouettes, health bars, target brackets, and objective marker
   are persistent.
2. The top strip contains only mission state, resources, power, supply, and an
   actionable raid warning.
3. A selected unit/structure gets one compact card. Its command card is
   contextual and short; longer explanations live in transient status text,
   the briefing, or the radio inbox.
4. Portrait transmissions are time-boxed and targetable. `Space` focuses the
   latest message, then the line yields the playfield back to the player.

## Mission pacing rules

- Open with a readable role job before the first raid.
- Give the player a safe resource route and a contested route with a reason to
  split the roster.
- Telegraph raids before spawning them; a warning is only useful if a player
  can move the Warden or Engineer in time.
- Let dialogue teach the next decision, but never gate an optional radio line
  in front of a required mission beat.
- Every new mission should provide a distinct terrain choice (cover,
  high-ground, flank, or choke) and a finite objective that can be tested in a
  renderer-free simulation.

## Reference reading

The interaction choices are informed by Blizzard's original StarCraft manual
and its current control guide: workers gather and return cargo, the command
console changes with the selected unit/building, hotkeys expose core commands,
control groups recall armies/buildings, and the minimap is a primary camera
surface.

- [StarCraft instruction manual (Blizzard PDF)](https://ftp.blizzard.com/pub/misc/StarCraft.PDF)
- [StarCraft II simplified controls (Blizzard)](https://news.blizzard.com/en-us/article/6640645/game-guide-simplified-controls)
- [StarCraft II special control guide (Blizzard)](https://news.blizzard.com/en-us/article/4552955/game-guide-special-control)

When a new feature is proposed, it should strengthen one of the contracts
above without increasing the persistent HUD footprint during normal play.
