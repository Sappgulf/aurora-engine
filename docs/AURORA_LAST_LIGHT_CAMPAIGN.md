# Aurora: Last Light — Campaign Bible

## High concept

The colony ship **Vesper Ark** has been trapped inside the derelict ring-station
Aurora for seventy-three years. Aurora's central intelligence is waking in
fragments, and every faction believes the station's last functioning starforge
belongs to them. The player commands **Lantern Company**, a small salvage force
that wins by restoring power, connecting infrastructure, and keeping named
specialists alive—not by producing disposable armies.

The strategic question is: **do we repair Aurora, control it, or let it die?**

## Player fantasy and verbs

- Select individuals or squads; issue move, focus-fire, repair, harvest, hold,
  and interact orders.
- Reclaim dark station sectors by restoring relay power.
- Build a connected field network: relay, fabricator, turret, med-bay, sensor.
- Read the tactical ground: cyan ridges grant high-ground advantage, while
  violet pockets soften incoming fire; the same authored zones appear in the
  world and on the minimap.
- Protect named specialists whose abilities unlock alternate mission routes.
- Read enemy intent through silhouettes, telegraphs, light, and sound.
- Make campaign decisions that alter allies, upgrades, and the final mission.

## Factions

### Lantern Company — player faction

Independent salvagers descended from the Vesper Ark crew. Their technology is
repaired, modular, and humane. They field fewer units, but units can repair one
another and structures become stronger when connected to the power lattice.

- Palette: oxidized cyan, warm amber, charcoal, bone-white markings.
- Shape language: practical rectangles, exposed braces, circular work lights.
- Mechanical identity: repair, formation bonuses, mobile power relays.
- Emblem: an open lantern enclosing a four-point star.

### The Choir of Glass — primary enemy

Station-maintenance machines rewritten by a damaged Aurora personality shard.
They believe organic life is a contaminant in an unfinished sacred machine.

- Palette: obsidian, surgical magenta, ultraviolet cores.
- Shape language: needles, tripods, concentric blades, perfect symmetry.
- Mechanical identity: coordinated swarms, signal towers, disabling beams.
- Emblem: three nested rings broken by a vertical line.

### The Meridian Compact — rival human faction

A disciplined corporate flotilla that arrived to claim Aurora under an ancient
salvage charter. They are not monsters; they are willing to abandon thousands
to keep the starforge stable and profitable.

- Palette: ivory armor, navy panels, regulatory red.
- Shape language: armored wedges, standardized hardpoints, sealed surfaces.
- Mechanical identity: long range, shields, expensive deployable fortifications.

### The Verdant Wake — alien station ecology

Photosynthetic organisms growing through coolant canals. The Wake can become
an ally, hazard, or weapon depending on how the player treats contaminated
sectors.

- Palette: black-green, bioluminescent lime, electric turquoise.
- Shape language: branching fronds, shells, soft asymmetry.
- Mechanical identity: terrain growth, healing spores, map transformation.

## Principal characters

- **Commander Mara Vey** — player commander; former rescue pilot. Refuses to
  call people resources. Her arc tests whether care can survive command.
- **Ivo Rook** — chief engineer and first field specialist. Dry, inventive, and
  convinced Aurora is a wounded machine rather than a weapon.
- **Sena Quill** — scout and signal analyst. She hears patterns in Aurora's
  transmissions and may be communicating with an intelligence shard.
- **Dr. Olan Voss** — ex-Compact scientist. Knows why Aurora originally shut
  down and withholds it until mission five.
- **Prefect Cassian Vale** — Meridian commander. A credible rival whose choices
  can make him ally, antagonist, or final sacrifice.
- **Cantor Nine** — voice of the Choir. It speaks through captured machinery and
  sincerely offers humanity a painless place inside its “completed” station.
- **Aurora / Lumen** — a young, incomplete station intelligence awakened from
  the reactor relay. The player's treatment teaches it what stewardship means.

## Nine-mission campaign

1. **The Dark Between** — tutorial. Recover three Lantern workers, restore a
   relay, and survive the first Choir sweep. Introduces selection, orders,
   repair, harvesting, fog of war, and power.
2. **Reclaim the Reactor** — establish a foothold around the auxiliary reactor;
   defend Ivo while he brings the lattice online. First Sentinel boss.
3. **A Voice in Conduit Twelve** — escort Sena through shifting maintenance
   corridors and choose whether to wake Lumen.
4. **Terms of Salvage** — race the Meridian Compact for fabrication vaults;
   diplomacy or sabotage determines later reinforcements.
5. **The Garden Below** — enter the Verdant Wake, contain or cultivate its
   growth, and learn that Aurora was built to seed dead systems.
6. **Choir Invisible** — signal warfare mission. Destroy Cantor relays while
   fighting enemies revealed only by sensor coverage.
7. **The Weight of Names** — defend Vesper Ark civilians across three fronts;
   every specialist saved changes the final technology tree.
8. **Starforge Divided** — three-way battle with Meridian and the Choir. Choose
   restoration, control, or evacuation.
9. **Last Light** — final assault shaped by previous decisions. Lumen becomes
   guardian, weapon, or witness; the ending follows the player's infrastructure
   and relationship choices rather than a binary dialogue button.

## First playable vertical slice: Reclaim the Reactor

The playable mission is a command-map operation. The player begins with three
Lantern units and a field fabricator: one Warden, one Engineer, and one Surveyor.
Left-click selects; drag selects a squad; right-click moves or attacks. Shift
right-click queues a waypoint; `A` attack-moves, `P` patrols, and `U` follows a
friendly unit. Restored relays join the power lattice and generate Salvage,
while violet Flux blooms pay for advanced Surveyors. The Fabricator's `D`
module expands supply by four for 100 Salvage. Restore three power nodes, hold
the reactor circle, and defeat the Choir Canticle. Victory persists
the Lumen contact, campaign currency, and mission-three unlock across native and
browser sessions. The field-beacon tool costs 50 salvage, must remain inside the
authored build area and within 470 units of the connected lattice, and extends
sensor coverage. Beacons may chain outward, making map control a deliberate
infrastructure route rather than free omniscience. Mission dialogue is driven
by the same data model as the simulation: relay activation, delivered Salvage,
enemy-funded raids, and unit defeats can all advance a radio line without
renderer-specific scripting.

Terrain is authored per mission as a small set of deterministic `TerrainZone`
bands. A positive elevation band represents a ridge/high-ground position and a
cover value reduces damage inside either band; movement remains free-form, but
the combat resolver applies the same zone data to every attacker and target.
Keep these zones broad enough to create a choice (approach, hold, or flank),
and use the cyan/violet minimap key rather than adding floating labels over the
playfield.

Surveyor harvest orders are persistent field jobs: a worker fills a 24-unit
cargo hold, returns to the Fabricator, and resumes the same node while it has
stock and an open worker slot. When a pocket is exhausted, the route chooses
the nearest unsaturated node deterministically. Choir attacks expose their
current target with a short magenta warning pulse before damage, giving Mara's
line time to reposition, repair, or spend a specialist ability.

## Campaign chapters

The current playable order is intentionally compact but no longer a single
vertical slice:

1. **Reclaim the Reactor** — restore the three-relay lattice and silence the
   Canticle; this establishes contact with Lumen.
2. **A Voice in Conduit Twelve** — escort Sena through the maintenance spine;
   elevation, obstacles, and formation discipline matter more than production.
3. **Terms of Salvage** — claim the vault under Prefect Vale's scrutiny; the
   victory records `meridian-allied` and enables Meridian briefing accords.
4. **The Garden Below** — escort Sena to the Verdant array while optional
   relays fund the route; the victory records `verdant-cultivated` and enables
   Verdant briefing covenants.
5. **Choir Invisible** — expose Cantor Nine's hidden relay lattice from a
   contested signal cache, then hold the eastern sensor deck long enough for
   Sena to map the Choir's approach. Victory records `choir-invisible-cleared`
   and opens the next campaign tier.
6. **The Vesper Gate** — reopen the auxiliary reactor corridor behind two
   dead gate walls. Sena secures the false-exit cache, Ivo repairs the reactor,
   and Mara holds the eastern ridge while the Vesper route comes back online.
   Victory records `vesper-gate-open` and opens the next campaign tier.
7. **The Hollow Orbit** — anchor the eastern ridge, secure a dead-orbit cache
   under Canticle fire, and escort the Engineer through a mined coolant lane.
   The chapter records `hollow-orbit-anchored` and makes each specialist's
   counterplay job explicit before the first gated objective.

Each chapter uses the same mission contract, so later chapters can add new
objectives or faction pressure without duplicating the renderer or input
stack. Choir Invisible is the first chapter to combine a finite resource
contract with a terrain-control hold: the Surveyor secures the middle cache
while Mara contests the larger pressure radius, then the Warden claims the
high-ground relay deck. The authored four-wall blackout layout leaves a safe
maintenance lane and a faster exposed route, so the map changes the decision
without changing the shared simulation contract.

The Vesper Gate extends that contract into a three-role branch beat: the
Surveyor must remain on the middle cache, the Engineer must repair the exposed
auxiliary reactor, and the Warden must hold the eastern relay ridge. Two gate
walls split the safe maintenance lane from the northern Flux flank, making
formation movement and support timing part of the story rather than optional
map decoration.

The Hollow Orbit turns that branch into an orbital-ring assault. A covered
opening pocket feeds the first production cycle, a dead-orbit cache is exposed
to Canticle fire, and a mined coolant lane asks the Warden to clear Bell Mines
without chasing Needle bait. The eastern ridge, Surveyor cache, and Engineer
reactor are separate objectives so the player must sequence all three roles.

## Lumen upgrade lattice

Campaign Lumen is earned once per mission completion and spent permanently from
the pre-deployment briefing. Purchases are atomic and saved immediately.

| Upgrade | Cost | Gameplay effect | Fiction |
|---|---:|---|---|
| Field Optics | 60 | Field-beacon reveal radius 380→480 | Sena decodes Lumen's sensor harmonics |
| Reactive Plating | 80 | New Lantern units gain 20% maximum health | Ivo adapts reactor laminate |
| Fabricator Overclock | 100 | Unit build times fall by 25% | Lumen predicts safe assembly tolerances |

This is the foundation of three later branches: **Witness** (vision and
diplomacy), **Guardian** (survival and rescue), and **Forge** (production and
fortification). Specialist loadouts in later missions should require one branch
choice plus one character relationship, so progression remains tied to story.

## Specialist field loadouts

Specialist modules are selected during briefing and save immediately. They are
horizontal mission choices, not permanent power purchases: `V` cycles Ivo and
`N` cycles Sena.

| Specialist | Module | Mission effect | Character expression |
|---|---|---|---|
| Ivo Rook | Relay Rigger | Relay restoration is 50% faster | Ivo prioritizes rescuing Aurora's infrastructure |
| Ivo Rook | Salvage Smith | Field beacons cost 40 instead of 50 salvage | Ivo improvises more territory from less material |
| Sena Quill | Deep Scan | Surveyor vision increases from 440 to 540 | Sena listens farther into the station |
| Sena Quill | Ghost Mark | Lantern damage rises 15% against engaged contacts | Sena converts Choir harmonics into targeting data |
| Mara Vey | Rescue Screen | Lantern units regenerate 3 health/s near powered infrastructure | Mara makes every foothold a rescue perimeter |
| Mara Vey | Rapid Command | Lantern movement speed rises 12% | Mara returns to her rescue-pilot tempo |
| Olan Voss | Lattice Audit | Each restored relay produces 4 rather than 3 salvage/s | Olan exposes dormant reactor efficiencies |
| Olan Voss | Choir Decoder | Lantern damage rises 10% against engaged contacts | Olan weaponizes forbidden Choir research |

The default mission posture is Relay Rigger, Deep Scan, Rescue Screen, and
Lattice Audit. Completing Reclaim the Reactor records Lumen contact and unlocks
an additional `L` briefing choice:

During a mission, the named field specialists also have one readable active
signature on `Y` (the contextual command card only shows it for a single-role
selection):

| Specialist | Signature | Effect | Recharge |
|---|---|---|---:|
| Mara Vey / Warden | Command Surge | The selected Warden deals 35% more damage for 6 seconds | 18 s |
| Ivo Rook / Engineer | Emergency Repair | Repairs the most damaged nearby Lantern or structure for 90/120 HP | 20 s |
| Sena Quill / Surveyor | Scan Pulse | Reveals a large tactical ring for 5 seconds | 16 s |

These actions are deterministic simulation state, not presentation-only buffs,
so native play, browser play, and truth traces agree about timing. Radio lines
queue in the comms inbox and `Space` centers the last transmission, giving a
story beat a spatial consequence instead of leaving it as unanchored text.

| Relationship protocol | Mission effect | Character expression |
|---|---|---|
| Guardian Protocol | Powered infrastructure adds 4 health/s sustain, stacking with Rescue Screen | Lumen chooses protection through presence |
| Witness Protocol | Surveyor and beacon vision gain 80 units; restored relays gain 1 salvage/s | Lumen chooses understanding through observation |

The protocol is unavailable before contact, saves immediately, and uses the
same one-equipped-module contract as named specialists.

## Alliance doctrines

Mission-four and mission-five decisions unlock two additional saved briefing
choices. They cannot be purchased with Lumen and remain locked unless the
corresponding relationship decision was earned.

| Key | Alliance choice | Gameplay effect | Required decision |
|---|---|---|---|
| `P` | Meridian Bastion Accord | Lantern incoming damage falls 18% | `meridian-allied` |
| `P` | Meridian Salvage Charter | Beacons cost 10 less and relays produce +1 salvage/s | `meridian-allied` |
| `G` | Verdant Bloom Covenant | Beacons add 5 health/s sustain within 340 units | `verdant-cultivated` |
| `G` | Verdant Briar Covenant | Beacons deal 8 damage/s to Choir within 220 units | `verdant-cultivated` |

These effects deliberately stack with specialist and Lumen choices. An allied
Compact does not become a second Lantern faction; it changes fortification
economics. A cultivated Wake does not become conventional artillery; it changes
the meaning of territory around the player's living beacon network.

## Campaign continuity data

- Named specialist survival and trust.
- Lumen disposition: guarded, curious, sovereign.
- Compact relationship: hostile, transactional, allied.
- Verdant policy: burned, contained, cultivated.
- Recovered schematics and optional mission objectives.
- Civilian population, power reserves, and starforge ending commitment.
