# Direct Control release scope

This file records the current functionality-first scope and implementation priority so deferred systems do not drift back into the critical path.

## Implementation priority

1. **Hold/Stop** — basic explicit stop only. `H` is the current test binding because native `S` pauses match presentation.
2. **Auto-attack chase timing** — observe vanilla AI chase/attack recovery and mirror the game's own movement-resume cadence instead of inventing an orb-walk timer.
3. **`End` — temporary release to AI / return to spectator**
   - release only the currently controlled champion;
   - restore normal AI control immediately;
   - do **not** release or accelerate pre-simulation;
   - `Ctrl+End` remains the stronger/global release.
4. **`A` — attack-move** — important ranged-character micro and especially important to Gunfighter.
5. **Single-target skill chase + follow-up attack**
   - when a targeted skill is ordered on an out-of-range legal target, retain that exact target and move toward it until the skill becomes legal;
   - cast the skill as soon as it becomes legal;
   - after the skill connects/executes, transition into the same exact-target auto-attack chase behavior so melee characters can continue pursuing a fleeing target without extra clicks.
6. **Minimap movement** — RMB/click movement through the minimap should issue a normal move order to the corresponding map location, allowing camera-independent travel.
7. **MOBA-style camera movement** — add practical camera controls for active pursuit, preferably matching familiar LoL behavior: configurable movement bindings and/or edge-of-screen scrolling.
8. **Clicks beyond the playable map edge** — battlefield clicks just outside the legal map should still express the intended direction. Clamp/project the request onto a legal/pathable map-edge destination so pathfinding moves the champion toward that edge rather than silently eating the order. Never allow movement off-map.
9. **Skill cooldown UI** — expose direct-control-friendly Q/W/R cooldown/readiness information with high visibility so the player does not have to infer cooldowns from the normal spectator presentation.
10. **Pings/team commands** — potentially large subsystem; do after the core direct-control command vocabulary and QoL are stable.
11. **Shop control** — automatic shop remains acceptable until this stage.

## Must iron out before release

- **Locked-skill safety:** never probe or emit Q/W/R inputs before the slot is unlocked for the selected champion level. Current progression gate: Q level 1, W level 3, R level 5. Physical testing found that probing a locked skill could wedge the watched simulation worker; the current build now rejects locked slots before validation and has passed the reproduction test.
- **Auto-attack chase recovery timing:** do not invent our own orb-walk/reset cadence. Observe vanilla AI simply chasing a moving enemy and basic-attacking, identify when vanilla resumes movement after an attack, and mirror that timing/cancel window. The current conservative full-cooldown hold is visibly too slow.
- **Dynamic skill targeting presentation:** max-range radials/ray clipping must reflect the current live match values after simulated balance patches. Never hard-code per-champion range values from one patch.
- **Basic Return behavior:** B uses the game's native Return Home input. Current repeated Return input immediately restarts recall after damage interruption; acceptable for functionality, but channel/restart behavior is a polish candidate before final release if it remains visually or mechanically undesirable.
- **Native-hook version resilience:** before 1.0, harden version-sensitive native discovery. Prefer masked/pattern scanning plus structural validation over fixed RVAs; use executable version/hash as diagnostics rather than the only locator; derive related call targets dynamically where practical; reject zero/ambiguous matches safely; and degrade only the affected feature if a private game layout can no longer be verified.

## Current Hold/Stop behavior

`H` cancels the selected champion's current move, exact-target attack, Return Home order, and armed skill targeting. Hold is an explicit persistent command. Ordinary movement/attack stops by anchoring to a fixed position. Because TFM2 does not treat a zero-distance MoveTo as an interrupt to an active Return channel, the current implementation emits one ordinary movement tick when Hold replaces Return, then captures and holds the champion's resulting position on the next tick. No retaliation or autonomous target acquisition is attached to Hold.

## Companion mod / Workshop launch note

**Anti-Freeze Fix** (Steam Workshop item `3800058476`) is currently considered a **highly recommended companion mod** for Teamfight Manager 2 v0.5.8-era Direct Control testing. Static inspection of its DLL indicates that it guards a native AI battle-planner expected-damage division where a zero estimate can otherwise wedge the simulation worker.

Causality is not yet proven: at least one full Direct Control match completed without interruption while Anti-Freeze Fix was disabled, and another completed with it enabled. Keep monitoring for freezes with the companion enabled. Recommendation is based on the technical relevance of the guarded native failure path as well as observed freeze symptoms, not on a completed controlled proof.

Do not silently copy or bundle that mod's implementation. For the eventual Workshop description, explicitly credit/shout out the Anti-Freeze Fix author and recommend the companion unless later game updates make it unnecessary.

## Deferred / later polish

- **Friendly champion selection cleanup:** do not redesign selection here. F1-F10 is sufficient for functionality; other mods can restrict scope or presentation.
- **Gunfighter move-while-attacking composition:** character-specific follow-up after generic controls are stable; attack-move is particularly important to this character.
- **Idle retaliation:** possible later behavior where an otherwise-idle selected champion that is attacked by an enemy already in legal basic-attack range returns fire.
- **Native Shortcuts-menu integration:** desirable final UX, but stable API currently provides no trivial registration hook.
- **Morgard/ping override investigation:** revisit only if explicit manual orders are still observably overridden after the core command path is stable.
