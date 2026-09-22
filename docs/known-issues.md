# Known issues and deferred polish

## Unfocused raw-key polling

Status: **pre-release fix required**.

Direct Control currently polls several keyboard controls with Win32 `GetAsyncKeyState`, which is global rather than scoped to the Teamfight Manager 2 foreground window. Physical testing confirmed that using `Ctrl+End` in another application while TFM2 remains open can silently trigger the mod's global release.

The focus-safety sweep must cover Ctrl+Home, Ctrl+End, End, F1-F10, and the worker-thread prematch Ctrl+Home escape. Mouse/MMB paths already perform a foreground-process check; SDK `key_pressed` controls are not part of this raw-key issue.

## Enemy follow can reveal a fogged champion

Status: **accepted first-release limitation**.

Automatic team fog is physically validated, but TFM2's native spectator follow behavior can still follow an opposing champion and thereby reveal that champion's position through fog. Direct Control will not attempt to turn spectator fog into an anti-cheat boundary for the first Workshop release.

This document records issues that are intentionally **not** blocking continued command work.

## Startup pre-simulation

Status: **one bounded pre-release attempt remains; defer and ship if unresolved**.

Teamfight Manager 2 requires some Candidate-A simulation progress before the Start Match transition can complete. Two startup-gating experiments were physically rejected:

- holding Candidate A inside its earliest observed AI callback freezes Start Match and can make Windows report the process as not responding;
- allowing one complete simulation tick and then holding is still too early for the battlefield to become independent of simulation progress.

The current fail-safe startup gate releases into the proven 60 Hz pacer after a short bounded hold. This avoids hangs, but the game still begins with some pre-simulated lead and can visibly progress before the player would ideally have made manual-control decisions.

This is annoying and visually inelegant, but it does **not** block live control once the match is running. The release-week plan allows one bounded attempt after the immediate buglist, preferably reusing a clean readiness/startup result from the Flame Simulator investigation. If that attempt does not produce a clean fix, document the remaining lead and ship the first public release anyway.

Post-release work may probe a later readiness boundary where the loader no longer depends synchronously on Candidate A. Do not allow startup work to destabilize the already-validated real-time pacing path.

## Cursor/world marker origin offset

Status: **known, non-blocking for coarse movement; blocking for precision clicking**.

The yellow world-space marker is consistently displaced from the physical mouse reticle. The displacement remains approximately constant across zoom levels, which strongly suggests that the scale is correct and the reference origin/viewport center is wrong.

Current diagnostics report `origin center_log`; the transform still uses `ingame.center_log` as a calibration reference. That node is not guaranteed to coincide with the real `Game` draw-map origin.

This must be corrected before precise actor/entity hit testing is considered production-ready. It does not invalidate the already-proven RMB MoveTo command path.

## Playback-speed controls during live pacing

Status: **immediate release-week buglist**.

While Candidate A is held near 60 Hz, the game's normal 0.5x/1x/1.5x/2x/3x replay controls do not meaningfully change live presentation speed. The UI may remember/display another speed, but the visible match effectively hugs the live simulation edge at about 1x.

Before the first public release, Direct Control should synchronize the presentation rate and Candidate-A pacer rate together. The same work includes the preferred death-timer fast-forward behavior. Broader playback-desync watchdog/settings polish may move post-release under `docs/release-week-plan.md`.

## Persistent MoveTo state

Status: **diagnostic behavior retained intentionally for now**.

The current movement command is persistent. A selected champion continues trying to reach the stored destination until another command replaces it. Runtime testing showed that this can survive pause/resume and even death/respawn.

That persistence is useful evidence that the command bridge is stable, but final MOBA-like behavior will probably clear or supersede movement on events such as arrival, death/respawn, attack orders, casts, or explicit stop/idle commands.

## Entity clickability branch

`feat/cursor-entity-picking` was created before the current athlete-aware selection and pacing architecture matured. Its entity-picker concepts remain useful, but the branch should **not** be merged wholesale.

When actor/entity targeting becomes the current task, transplant or rebase the picker logic onto the current live-control architecture. Expect manual integration in at least `src/lib.rs` and `src/control.rs`.
