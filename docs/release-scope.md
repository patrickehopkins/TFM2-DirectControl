# Direct Control release scope

This file records the current functionality-first scope and implementation priority so deferred systems do not drift back into the critical path.

## Release boundary

**Pings/team commands are the release boundary.** The intent is to ship the first public Direct Control release after the control/QoL work immediately before Pings is complete and validated, rather than holding release for the potentially larger ping-behavior subsystem. Unless explicitly stated otherwise, any new pre-release jobs added from this point should be inserted immediately before Pings/team commands.

## Implementation priority

1. **Hold/Stop** — **validated.** `H` is the current binding because native `S` pauses match presentation. It stops movement, attacking, skill aim, and Return/recall while retaining manual ownership.
2. **Auto-attack chase timing** — **validated strongly.** When an exact-target Attack is temporarily illegal, Direct Control asks TFM2 whether literal `MoveTo(target)` is legal on that exact simulation frame. Legal movement resumes pursuit immediately; illegal movement falls back to hold.
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
   - native TFM2 `A = Back 10 Seconds` must be remapped during development unless/until shortcut integration prevents the conflict.
5. **Single-target skill chase + follow-up attack** — **validated.**
   - a confirmed hostile Target skill that is currently out of range retains the exact clicked entity and moves toward it until the skill becomes legal;
   - runtime validator evidence is used conservatively to distinguish hostile Target skills from Direction/Position/Self actions;
   - cast immediately once the exact Target input becomes legal;
   - after a successful hostile single-target cast, transition that same entity into exact-target auto-attack/chase behavior;
   - if the target dies, becomes untargetable, changes relation, or leaves legal vision before the cast, drop the pending chase rather than tracking through fog;
   - locked-slot validation remains ahead of all skill probing.
6. **Minimap movement** — **validated in both live UI layouts.**
   - recognize only the minimap belonging to the currently active match layout;
   - contextual RMB is preserved: ground issues MoveTo and hostile markers issue exact-target Attack;
   - minimap input is camera-independent and does not fall through to ordinary battlefield projection;
   - current armed-skill/minimap ray behavior is acceptable and intentionally unchanged for now.
7. **Universal skill use while on cooldown** — **validated.**
   - while Q/W/R is on cooldown, pressing/confirming it produces **no gameplay response**;
   - do not arm/ray-cast, queue chase, or queue a delayed cast;
   - later cooldown UI should make the unavailable state explicit without changing the no-response gameplay rule.
8. **MOBA-style camera controls** — **validated / locked for now.**
   - MMB grab-and-drag is physically validated with the intended 1:1 cursor-driven feel;
   - MMB continues normally across the minimap, player cards, buttons, and top/bottom native UI; the MMB/UI integration issue is **PASS**;
   - match-wide mouse-wheel zoom is **PASS**: wheel up zooms in, wheel down zooms out, using the native `0.25` step and `0.5 .. 3.0` limits;
   - MMB and wheel zoom remain active after `End` returns the champion to AI/spectator mode;
   - preserve the validated native-pan / narrow MMB UI-routing architecture and do not modify it during unrelated work;
   - **screen-edge scrolling is shelved** and all Harbinger edge-scroll behavior remains removed from the active driver;
   - arrow-key camera panning was rejected and removed;
   - native minimap LMB camera relocation + RMB command workflow remains intact;
   - detailed architecture and rejected experiments are recorded in `docs/camera-controls.md`.
9. **Clicks beyond the playable map edge** — **validated.** Battlefield clicks beyond the legal map express direction by clamping only the requested movement destination into the legal 0..960 world square. TFM2 still receives an ordinary movement request, so native pathfinding, terrain, champion radius, and entity collision remain authoritative; Direct Control never grants permission to leave the map.
   - physical validation passed RMB and attack-move edge/corner movement without bypassing normal pathing/collision.
   - current rapid-fire queue: **automatic team fog-of-war -> F-key selection hardening -> synchronized match speeds/death fast-forward -> click-target hitbox polish**; then revisit the remaining order.
10. **Skill cooldown UI** — expose direct-control-friendly Q/W/R cooldown/readiness information with high visibility so the player does not have to infer cooldowns from the normal spectator presentation. Include explicit red feedback for attempted use while unavailable.
11. **Click-target hitbox polish**
   - enlarge only the **clickable/selectable area**; never alter entity collision/pathing geometry;
   - towers and the final objective need substantially more forgiving selection;
   - champions should receive modest click forgiveness, especially to reduce rapid-RMB attack orders accidentally becoming ground MoveTo orders;
   - creeps should receive only enough forgiveness to remain usable without making the lane visually sticky;
   - when enlarged selectable areas overlap, prefer **Champion > Building/Objective > Creep**.
12. **Max-range skill radii / ray clipping revisit**
   - radial/ray range must represent current live match values after simulated balance patches;
   - do not hard-code per-champion ranges from one game patch;
   - investigate deeper live action/effect metadata or a version-resilient native extraction route if the stable runtime API remains insufficient.
13. **Automatic team fog-of-war on direct control** — **implementation route under investigation.**
   - the authoritative Candidate-A callback now publishes the selected champion's actual simulation team; do not infer side from F-key position or the user's original team;
   - **rejected:** synthesizing default `X` / `C` keypresses. Automatic fog is semantic behavior, not a shortcut, and must not depend on the player's configurable key bindings;
   - preferred route: invoke the native `in_game_camera_team0` / `in_game_camera_team1` action directly, or manipulate a separately verified native spectator-vision state if no semantic action-call surface is available;
   - completed diagnostic: the captured native camera `mode` remained `0` for All / Team 0 / Team 1 vision, so that field is **not** the fog/vision selector and must not be repurposed;
   - switching to the opposite-side champion must switch fog to that champion's team; `End` should not forcibly change fog unless we later choose that explicitly.
14. **F-key selection mapping hardening**
   - preserve the game's native role order exactly after verifying it during implementation; current working hypothesis is `F1-F5 = player team Top, Jungle, Mid, Bottom, Support` and `F6-F10 = opponent team` in that same order;
   - current implementation finds visible `(F1)`-`(F10)` player cards in the UI tree and name-matches them to stable athlete ids; this works but is indirect and UI-layout-sensitive;
   - investigate piggy-backing the base game's existing Follow Own / Follow Enemy role tracking;
   - A/B test against the working UI/name mapper and retain the existing mapper if the native route introduces complications.
15. **Custom Direct Control keybinds / shortcut-mode separation** — full design inventory is in `docs/keybind-plan.md`.
   - preferred design: dedicated **Direct Control** shortcut category/control scheme active only while a champion is manually controlled; native spectator shortcuts resume immediately when `End` releases control;
   - acceptable fallback: normal shortcut settings namespace with conflict handling and clear labeling;
   - rejected design: globally override native shortcuts regardless of mode;
   - catalogue all injected player-facing actions and camera gestures;
   - **playback-desync safety is mandatory:** suppress native commands that seek/jump/pause presentation away from the live controlled simulation, including Back/Forward 10 Seconds and Previous/Next Highlight;
   - ordinary speed controls are an exception only when Harbinger synchronizes presentation speed and live simulation pacing together;
   - **snap-to-live watchdog is mandatory:** detect divergence between presentation/playback position and live paced simulation while Direct Control owns a champion, snap presentation directly back to live, and restore the selected synchronized speed;
   - do **not** repair desync by temporarily speeding playback until it catches up;
   - prefer event-driven detection from the native playback controller; lightweight polling is the fallback.
16. **Current-gold HUD** — pre-release readability polish.
   - expose the manually controlled champion/player's current spendable gold somewhere continuously readable in Direct Control mode;
   - support both live match UI layouts;
   - richer next-purchase information belongs to the later shop-control/auto-shop refinement in `docs/economy-ui-plan.md`.
17. **Synchronized match-speed variation / death fast-forward** — pre-release playback QoL.
   - change Candidate-A wall-clock pacing and presentation rate together;
   - requested mappings: `0.5x = 30 Hz`, `1x = 60 Hz`, `1.5x = 90 Hz` if exposed, `2x = 120 Hz`, `3x = 180 Hz`;
   - re-anchor pacing immediately whenever speed changes so old-rate elapsed time never becomes catch-up/slowdown budget;
   - Highlight Mode itself must not seek/skip presentation during Direct Control;
   - preferred Highlight replacement is **death-timer fast-forward**: while the controlled champion is dead, run live simulation and presentation together at a deliberately fast rate, then restore the prior ordinary speed on respawn;
   - cancel/restore the prior speed if control is released or switched to a living champion before respawn.
18. **Pregame / pre-simulation start handling**
   - preferred route: reuse any clean solution discovered by the Flame Simulator pre-game pre-simulation probe so the player can enter the map before meaningful simulation gets ahead of them;
   - fallback: apply a flat **+60 second offset** to the normal opening schedule while preserving all relative event timing;
   - shift character AI activation, lane waves, jungle spawns, Serpen/Morgar, and other opening schedule events together; do not alter ordinary combat cooldown/action timing;
   - optionally add a temporary spawn-area barrier only if unrestricted first-minute roaming proves abusive.
19. **Champion-specific compatibility sweep** — pre-release, universal-first. See `docs/champion-compatibility.md`.
   - **Gambler Skill 1:** confirmed completely nonfunctional under Direct Control. Identify whether it belongs to a broader native action/target family first; use an isolated Gambler adapter only if its skill is genuinely unique.
   - **Gunfighter attack-move:** confirmed that `A + LMB` currently produces either attacking or walking rather than his intended move-while-attacking behavior. Revisit `can_use_with_move` and native move-compatible attack semantics first; add unique handling only if required.
   - do not distort already validated generic champion behavior to accommodate one unusual champion.
20. **Pause/menu and early-results safety audit** — pre-release playback integrity.
   - reproduce long pause-menu states and verify Candidate A actually stops rather than presentation pausing while live simulation continues ahead;
   - resume must re-anchor pacing so pause duration cannot become catch-up budget;
   - inspect **View Match Results Immediately** while simulation is still in progress;
   - if that control forces completion, seeks presentation, or bypasses pacing, gate or synchronize it while Direct Control owns the live match.
21. **Resolution / UI-scaling compatibility audit** — final pre-release portability polish immediately before Pings.
   - current physical validation is concentrated on one native-resolution setup; explicitly test resolution, window size, aspect ratio, Windows DPI/display scaling, and any in-game UI-scale option;
   - cover both full and Info/split layouts and multiple 16:9 resolutions/window sizes; add a non-16:9 case if TFM2 supports one cleanly;
   - re-test RMB world projection, skill aim, minimap input, MMB 1:1 drag, MMB UI bypass, mouse-wheel zoom, HUD placement, and click-target geometry;
   - most gameplay projection already uses live `draw_map_size("UI")`, live `ingame.center_log`, and live minimap rectangles, but MMB still relies on the validated `1920 x 1080` logical-UI assumption outside `StableClient`, so treat it as the highest-risk scaling path;
   - if a mismatch is found, publish live UI/battlefield geometry from the stable client into the camera adapter rather than adding resolution-specific constants.
22. **Pings/team commands** — **post-release work / release boundary.** Potentially large subsystem; do not hold the first public release for this unless explicitly reconsidered.
23. **Shop control** — post-release/manual-shopping work. Default auto-shop remains an explicit supported mode; when auto-shop is selected, expose the native next intended item/upgrade and additional gold needed beside current gold where practical. See `docs/economy-ui-plan.md`.

## Must iron out before release

- **Locked-skill safety:** never probe or emit Q/W/R inputs before the slot is unlocked for the selected champion level. Current progression gate: Q level 1, W level 3, R level 5. Physical testing found that probing a locked W/R can wedge the watched simulation worker; the current build rejects locked slots before validation and has passed the reproduction test.
- **Dynamic skill targeting presentation:** max-range radials/ray clipping must reflect current live match values after simulated balance patches. Never hard-code per-champion range values from one patch.
- **Basic Return behavior:** B uses the game's native Return Home input. Current repeated Return input immediately restarts recall after damage interruption; acceptable for functionality, but channel/restart behavior remains a polish candidate.
- **Native-hook version resilience:** before 1.0, harden version-sensitive native discovery. Prefer masked/pattern scanning plus structural validation over fixed RVAs; use executable version/hash as diagnostics rather than the only locator; derive related call targets dynamically where practical; reject zero/ambiguous matches safely; and degrade only the affected feature if a private layout can no longer be verified.

## Current Hold/Stop behavior

`H` cancels the selected champion's current move, exact-target attack, Return Home order, and armed skill targeting. Hold is an explicit persistent command. Ordinary movement/attack stops by anchoring to a fixed position. Because TFM2 does not treat a zero-distance MoveTo as an interrupt to an active Return channel, the implementation emits one ordinary movement tick when Hold replaces Return, then captures and holds the champion's resulting position on the next tick. Physical testing confirms this closes the recall gap.

## Auto-attack chase timing note

The earlier conservative implementation held for the entire remaining basic-attack cooldown. That was visibly too slow. The current implementation no longer derives recovery from cooldown: after Attack becomes invalid, it validates literal chase movement on the current frame. If TFM2 accepts movement, pursuit resumes; if not, Direct Control holds.

Rapid RMB can still appear to "chase without attacking" because every new click is context-resolved independently. A click that misses the hostile entity's current selectable circle becomes ground MoveTo and replaces the prior Attack order. Treat that as click-target hitbox/selectability polish, not as a reason to disturb the validated chase-timing rule.

## Companion mod / Workshop launch note

**Anti-Freeze Fix** (Steam Workshop item `3800058476`) is currently considered a **highly recommended companion mod** for Teamfight Manager 2 v0.5.8-era Direct Control testing. Static inspection of its DLL indicates that it guards a native AI battle-planner expected-damage division where a zero estimate can otherwise wedge the simulation worker.

Causality is not yet proven: at least one full Direct Control match completed without interruption while Anti-Freeze Fix was disabled, and another completed with it enabled. Keep monitoring for freezes with the companion enabled. Recommendation is based on the technical relevance of the guarded native failure path as well as observed freeze symptoms, not on a completed controlled proof.

Do not silently copy or bundle that mod's implementation. For the eventual Workshop description, explicitly credit/shout out the Anti-Freeze Fix author and recommend the companion unless later game updates make it unnecessary.

## Deferred / later polish

- **Screen-edge camera scrolling:** shelved after repeated physical tests showed stationary-edge ticking and top/bottom UI blockage. Revisit only through a deeper native camera/update investigation; do not restore the rejected synthetic-mouse wake workaround.
- **Space recenter/follow:** desirable camera QoL, but not part of the now-locked camera milestone. Consider exposing native follow/recenter behavior later through the shortcut system.
- **Friendly champion selection cleanup:** do not redesign the visible selection UI here. F1-F10 is sufficient for functionality; only harden how those fixed role slots resolve to athlete ids.
- **Idle retaliation:** possible later behavior where an otherwise-idle selected champion that is attacked by an enemy already in legal basic-attack range returns fire.
- **Morgard/ping override investigation:** revisit only if explicit manual orders are still observably overridden after the core command path is stable.
