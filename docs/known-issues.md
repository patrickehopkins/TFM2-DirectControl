# Known issues and deferred polish

This document records issues that are intentionally **not** blocking continued command work.

## Startup pre-simulation

Status: **shelved for later polish**.

Teamfight Manager 2 requires some Candidate-A simulation progress before the Start Match transition can complete. Two startup-gating experiments were physically rejected:

- holding Candidate A inside its earliest observed AI callback freezes Start Match and can make Windows report the process as not responding;
- allowing one complete simulation tick and then holding is still too early for the battlefield to become independent of simulation progress.

The current fail-safe startup gate releases into the proven 60 Hz pacer after a short bounded hold. This avoids hangs, but the game still begins with some pre-simulated lead and can visibly progress before the player would ideally have made manual-control decisions.

This is annoying and visually inelegant, but it does **not** block live control once the match is running. Do not spend further implementation time on startup separation until higher-priority gameplay commands are working.

A future polish pass may probe a small startup runway (for example 2/5/10+ complete ticks) or identify a later readiness boundary where the loader no longer depends synchronously on Candidate A.

## Cursor/world marker origin offset

Status: **known, non-blocking for coarse movement; blocking for precision clicking**.

The yellow world-space marker is consistently displaced from the physical mouse reticle. The displacement remains approximately constant across zoom levels, which strongly suggests that the scale is correct and the reference origin/viewport center is wrong.

Current diagnostics report `origin center_log`; the transform still uses `ingame.center_log` as a calibration reference. That node is not guaranteed to coincide with the real `Game` draw-map origin.

This must be corrected before precise actor/entity hit testing is considered production-ready. It does not invalidate the already-proven RMB MoveTo command path.

## Playback-speed controls during live pacing

Status: **deferred**.

While Candidate A is held near 60 Hz, the game's normal 0.5x/1x/1.5x/2x/3x replay controls do not meaningfully change live presentation speed. The UI may remember/display another speed, but the visible match effectively hugs the live simulation edge at about 1x.

For the initial direct-control version, 1x is the intended live-control rate. Variable-speed live control should later change the Candidate-A pacer rate itself rather than relying on the replay-speed UI.

## Persistent MoveTo state

Status: **diagnostic behavior retained intentionally for now**.

The current movement command is persistent. A selected champion continues trying to reach the stored destination until another command replaces it. Runtime testing showed that this can survive pause/resume and even death/respawn.

That persistence is useful evidence that the command bridge is stable, but final MOBA-like behavior will probably clear or supersede movement on events such as arrival, death/respawn, attack orders, casts, or explicit stop/idle commands.

## Entity clickability branch

`feat/cursor-entity-picking` was created before the current athlete-aware selection and pacing architecture matured. Its entity-picker concepts remain useful, but the branch should **not** be merged wholesale.

When actor/entity targeting becomes the current task, transplant or rebase the picker logic onto the current live-control architecture. Expect manual integration in at least `src/lib.rs` and `src/control.rs`.
