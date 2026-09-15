# Direct Control release scope

This file records the current functionality-first scope and implementation priority so deferred systems do not drift back into the critical path.

## Release boundary

**Pings/team commands are the release boundary.** The intent is to ship the first public Direct Control release after the control/QoL work immediately before Pings is complete and validated, rather than holding release for the potentially larger ping-behavior subsystem. Unless explicitly stated otherwise, any new pre-release jobs added from this point should be inserted immediately before Pings/team commands.

## Implementation priority

1. **Hold/Stop** — **validated.** `H` is the current binding because native `S` pauses match presentation. It stops movement, attacking, skill aim, and Return/recall while retaining manual ownership.
2. **Auto-attack chase timing** — **validated strongly.** When an exact-target Attack is temporarily illegal, Direct Control now asks TFM2 whether literal `MoveTo(target)` is legal on that exact simulation frame. Legal movement resumes pursuit immediately; illegal movement falls back to hold. Physical testing showed this closely matches desired attack/chase cadence without inventing a cooldown or windup timer.
3. **`End` — temporary release to AI / return to spectator** — **validated.**
   - releases only the currently controlled champion;
   - restores normal AI control immediately;
   - clears retained manual move/attack/return/skill-targeting state;
   - does **not** release, skip, or accelerate pre-simulation/pacing;
   - `Ctrl+End` remains the stronger/global release.
4. **`A` — attack-move** — **validated.**
   - `A` arms attack-move and LMB confirms the destination;
   - move toward that point until a visible hostile becomes a legal basic-attack target;
   - acquire the nearest currently legal target to the controlled champion, then retain it through the normal exact-target attack/chase timing;
   - after the acquired target dies, becomes untargetable, or leaves vision, resume the original attack-move destination and allow a new legal target to be acquired;
   - do not auto-hunt distant visible enemies before they enter legal basic-attack range;
   - physical testing completed without complications;
   - native TFM2 `A = Back 10 Seconds` must be remapped during development unless/until shortcut integration can prevent the conflict.
5. **Single-target skill chase + follow-up attack** — **validated.**
   - a confirmed hostile Target skill that is currently out of range retains the exact clicked entity and moves toward it until the skill becomes legal;
   - runtime validator evidence is used conservatively to distinguish hostile Target skills from Direction/Position/Self actions rather than treating every failed entity click as a chase request;
   - cast the skill immediately once the exact Target input becomes legal;
   - after a successful hostile single-target cast, transition that same entity into the existing exact-target auto-attack/chase behavior without requiring another click;
   - if the target dies, becomes untargetable, changes relation, or leaves legal vision before the cast, drop the pending chase rather than tracking through fog;
   - locked-slot validation remains ahead of all skill probing.
6. **Minimap movement** — **validated in both live UI layouts.**
   - recognize only the minimap belonging to the currently active match layout rather than leaving both calibrated minimap hitboxes live simultaneously;
   - contextual RMB behavior is intentionally preserved: ground issues MoveTo and hostile markers issue exact-target Attack;
   - minimap input is camera-independent and does not fall through to ordinary battlefield projection;
   - current armed-skill/minimap ray behavior is acceptable and intentionally unchanged for now.
7. **Universal skill use while on cooldown** — **validated.**
   - while Q/W/R is on cooldown, pressing/confirming it produces **no gameplay response**;
   - do not arm/ray-cast a cooldown skill;
   - do not queue or begin single-target chase for a cooldown skill;
   - do not queue a delayed cast;
   - later cooldown UI should provide explicit red feedback/readability (for example `Q ON COOLDOWN`) without changing the no-response gameplay rule.
8. **MOBA-style camera movement** — **active polish / physical testing.**
   - essential gestures are mouse-edge scrolling and middle-mouse click-drag;
   - MMB/native-pan implementation is currently being calibrated for exact cursor anchoring at both slow and fast drag speeds;
   - edge scrolling needs reliable all-four-edge activation and useful speed;
   - arrow-key camera panning was physically rejected and removed;
   - add hold-Space recenter/follow after the two core camera gestures are stable; persistent follow/lock can later be exposed through the Direct Control shortcut scheme;
   - do not disturb the already-useful minimap LMB camera relocation + RMB command workflow.
9. **Clicks beyond the playable map edge** — battlefield clicks just outside the legal map should still express the intended direction. Clamp/project the request onto a legal/pathable map-edge destination so pathfinding moves the champion toward that edge rather than silently eating the order. Never allow movement off-map.
10. **Skill cooldown UI** — expose direct-control-friendly Q/W/R cooldown/readiness information with high visibility so the player does not have to infer cooldowns from the normal spectator presentation. Include explicit red feedback for attempted use while unavailable.
11. **Click-target hitbox polish**
   - enlarge only the **clickable/selectable area**; never alter entity collision/pathing geometry;
   - towers and the final objective need substantially more forgiving selection because their rendered footprint is much larger than their current clickable collision circle;
   - champions should receive modest click forgiveness as well, especially to reduce rapid-RMB attack orders accidentally becoming ground MoveTo orders;
   - creeps may receive only enough forgiveness to remain usable without making the lane visually "sticky";
   - when enlarged selectable areas overlap, prefer **Champion > Building/Objective > Creep**.
12. **Max-range skill radii / ray clipping revisit**
   - this remains high-value direct-control QoL and must be revisited rather than accepted as permanently blocked;
   - radial/ray range must represent the current live match values after simulated balance patches;
   - do not hard-code per-champion ranges from one game patch;
   - investigate deeper live action/effect metadata or a version-resilient native extraction route if the stable runtime API remains insufficient.
13. **Automatic team fog-of-war on direct control**
   - when manual control is taken of a champion, automatically switch spectator vision/fog-of-war to that champion's team;
   - this should follow whichever side the selected champion belongs to rather than assuming the user's original team.
14. **F-key selection mapping hardening**
   - preserve the game's native role order exactly after verifying it during implementation; current working hypothesis is `F1-F5 = player team Top, Jungle, Mid, Bottom, Support` and `F6-F10 = opponent team` in that same order;
   - current Direct Control implementation finds visible `(F1)`-`(F10)` player cards in the UI tree and name-matches them back to stable athlete ids; this works, but is more indirect and more UI-layout-sensitive than necessary;
   - investigate piggy-backing the base game's existing Follow Own / Follow Enemy role tracking so the same native team/role identity source drives manual selection;
   - A/B test the native-derived mapper against the existing UI/name mapper and revert/retain the existing mapper if the native route introduces complications;
   - prefer native/stable team-role data over repeated UI-tree scans if accessible; keep the existing mapper as a fallback.
15. **Custom Direct Control keybinds / shortcut-mode separation** — pre-release polish before match-start timing. Full design inventory is in `docs/keybind-plan.md`.
   - preferred design: add a dedicated **Direct Control** category/control scheme in Shortcuts Settings that becomes active only while a champion is under manual control; native spectator shortcuts resume immediately when `End` releases control;
   - acceptable fallback: place Direct Control entries into the normal shortcut settings namespace with conflict handling/clear labeling if a separate mode is disproportionately invasive;
   - rejected design: globally override native shortcut behavior with Direct Control defaults regardless of mode;
   - catalogue all injected player-facing actions and camera gestures;
   - **playback-desync safety is mandatory:** suppress native commands that seek/jump/pause presentation away from the live controlled simulation, including Back/Forward 10 Seconds and Previous/Next Highlight;
   - ordinary speed controls are a deliberate exception only when Harbinger synchronizes presentation speed and live simulation pacing together;
   - **snap-to-live watchdog is also mandatory:** independently detect divergence between the presentation/playback position and the live paced simulation while Direct Control owns a champion; if they differ beyond a small expected tolerance, immediately snap presentation back to the live point and restore the currently selected synchronized speed as needed;
   - do **not** repair desync by temporarily speeding playback until it catches up;
   - prefer event-driven desync detection from the native playback controller if available; otherwise use a lightweight periodic comparison. Disable the watchdog when `End` returns the player to ordinary spectator mode.
16. **Current-gold HUD** — pre-release readability polish.
   - expose the manually controlled champion/player's current spendable gold somewhere continuously readable in Direct Control mode;
   - support both live match UI layouts;
   - this is the minimal pre-release economy information; richer next-purchase information belongs to the later shop-control/auto-shop refinement in `docs/economy-ui-plan.md`.
17. **Synchronized match-speed variation / death fast-forward** — pre-release playback QoL.
   - allow useful in-game speed variation during Direct Control by changing the Candidate-A wall-clock pacer and presentation rate together;
   - requested mappings: `0.5x = 30 Hz`, `1x = 60 Hz`, `2x = 120 Hz`, `3x = 180 Hz`; if native `1.5x` remains exposed, synchronize it at `90 Hz` rather than allowing an unsafe presentation-only rate;
   - re-anchor pacing immediately whenever speed changes so time accumulated under the old multiplier never becomes catch-up/slowdown budget;
   - native Highlight Mode itself is not useful during Direct Control and must not seek/skip the presentation;
   - preferred replacement for the Highlight slot is a **death-timer fast-forward**: while the controlled champion is dead, temporarily run live simulation and presentation together at a deliberately fast rate, then automatically restore the player's previous ordinary speed on respawn;
   - if the native Highlight speed multiplier can be identified as a fixed safe rate, it may be reused for death fast-forward; otherwise choose an explicit rate rather than guessing;
   - cancel/restore the prior speed if control is released or switched to a living champion before respawn.
18. **Pregame / pre-simulation start handling** — final pre-release polish/fallback before Pings.
   - preferred route: reuse any clean solution discovered by the Flame Simulator pre-game pre-simulation probe so the player can enter the map before meaningful match simulation gets ahead of them;
   - if no clean start-gate/pause solution is found, apply a flat **+60 second offset** to the normal opening schedule rather than allowing the vanilla timings to occur before the player can meaningfully participate;
   - the fallback offset must preserve all normal relative timing: character AI activation, lane creep spawns/waves, jungle spawns, Serpen and Morgar spawns, and other scheduled opening events each occur one minute later than they normally would; do **not** bunch all of those events together at the 1:00 mark;
   - treat this as a match-start schedule offset, not a blanket modification to ordinary combat cooldowns/action durations;
   - the first minute can function as a League-like pregame roam/setup window before normal match activity begins;
   - optionally add a temporary spawn-area collision wall/barrier only if testing shows unrestricted first-minute roaming creates undesirable exploits. Prefer free roaming if it behaves well.
19. **Pings/team commands** — **post-release work / release boundary.** Potentially large subsystem; do not hold the first public release for this unless explicitly reconsidered.
20. **Shop control** — post-release/manual-shopping work. Default auto-shop remains an explicit supported mode; when auto-shop is selected, expose the native next intended item/upgrade and the additional gold needed to afford it beside current gold where practical. See `docs/economy-ui-plan.md`.

## Must iron out before release

- **Locked-skill safety:** never probe or emit Q/W/R inputs before the slot is unlocked for the selected champion level. Current progression gate: Q level 1, W level 3, R level 5. Physical testing found that probing a locked W/R can wedge the watched simulation worker; the current build now rejects locked slots before validation and has passed the reproduction test.
- **Dynamic skill targeting presentation:** max-range radials/ray clipping must reflect the current live match values after simulated balance patches. Never hard-code per-champion range values from one patch.
- **Basic Return behavior:** B uses the game's native Return Home input. Current repeated Return input immediately restarts recall after damage interruption; acceptable for functionality, but channel/restart behavior is a polish candidate before final release if it remains visually or mechanically undesirable.
- **Native-hook version resilience:** before 1.0, harden version-sensitive native discovery. Prefer masked/pattern scanning plus structural validation over fixed RVAs; use executable version/hash as diagnostics rather than the only locator; derive related call targets dynamically where practical; reject zero/ambiguous matches safely; and degrade only the affected feature if a private game layout can no longer be verified.

## Current Hold/Stop behavior

`H` cancels the selected champion's current move, exact-target attack, Return Home order, and armed skill targeting. Hold is an explicit persistent command. Ordinary movement/attack stops by anchoring to a fixed position. Because TFM2 does not treat a zero-distance MoveTo as an interrupt to an active Return channel, the implementation emits one ordinary movement tick when Hold replaces Return, then captures and holds the champion's resulting position on the next tick. Physical testing now confirms that this closes the recall gap as well.

## Auto-attack chase timing note

The earlier conservative implementation held for the entire remaining basic-attack cooldown. That was visibly too slow. The current implementation no longer derives recovery from cooldown at all: after Attack becomes invalid, it validates literal chase movement on the current frame. If TFM2 accepts the movement, pursuit resumes; if not, Direct Control holds. Physical testing produced a strong result and is now the preferred generic chase rule.

Rapid RMB can still appear to "chase without attacking" because every new click is context-resolved independently. A click that misses the hostile entity's current selectable circle becomes a ground MoveTo and replaces the prior Attack order. Treat that as click-target hitbox/selectability polish, not as a reason to disturb the validated chase-timing rule.

## Companion mod / Workshop launch note

**Anti-Freeze Fix** (Steam Workshop item `3800058476`) is currently considered a **highly recommended companion mod** for Teamfight Manager 2 v0.5.8-era Direct Control testing. Static inspection of its DLL indicates that it guards a native AI battle-planner expected-damage division where a zero estimate can otherwise wedge the simulation worker.

Causality is not yet proven: at least one full Direct Control match completed without interruption while Anti-Freeze Fix was disabled, and another completed with it enabled. Keep monitoring for freezes with the companion enabled. Recommendation is based on the technical relevance of the guarded native failure path as well as observed freeze symptoms, not on a completed controlled proof.

Do not silently copy or bundle that mod's implementation. For the eventual Workshop description, explicitly credit/shout out the Anti-Freeze Fix author and recommend the companion unless later game updates make it unnecessary.

## Deferred / later polish

- **Friendly champion selection cleanup:** do not redesign the visible selection UI here. F1-F10 is sufficient for functionality; only harden how those fixed role slots resolve to athlete ids.
- **Gunfighter move-while-attacking composition:** character-specific follow-up after generic controls are stable; attack-move is particularly important to this character.
- **Idle retaliation:** possible later behavior where an otherwise-idle selected champion that is attacked by an enemy already in legal basic-attack range returns fire.
- **Morgard/ping override investigation:** revisit only if explicit manual orders are still observably overridden after the core command path is stable.
