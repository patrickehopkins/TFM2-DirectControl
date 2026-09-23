# Known issues and deferred polish

## Unfocused raw-key polling

Status: **fixed and physically validated**.

All raw Win32 keyboard paths now share a foreground-process gate. Ctrl+Home, Ctrl+End, End, F1-F10, and the worker-thread startup Ctrl+Home escape are ignored unless Teamfight Manager 2 owns the foreground window. A key/chord held while focus returns is also swallowed until it is released and pressed again, preventing background shortcuts from firing on refocus.

Mouse/MMB paths already had their own foreground-window protection; SDK `key_pressed` controls remain contextual and are not part of this raw-key issue.

## Enemy follow can reveal a fogged champion

Status: **accepted first-release limitation**.

Automatic team fog is physically validated, but TFM2's native spectator follow behavior can still follow an opposing champion and thereby reveal that champion's position through fog. Direct Control will not attempt to turn spectator fog into an anti-cheat boundary for the first Workshop release.

This document records issues that are intentionally **not** blocking continued command work.

## Startup pre-simulation

Teamfight Manager 2 still requires watched-match simulation progress before the battlefield can be constructed. Physical v0.6.1 probing confirmed Candidate A is already the correct `ClientMatchView` simulation at tick 1, so the remaining lead is a loader dependency rather than misidentifying the simulation.

The release build handles this by freezing Candidate A at the first usable `InGame` boundary and withholding `Ctrl+Home` until the visible presentation catches that frozen live state. This prevents manual commands from targeting simulation state that the player has not yet seen.

The startup lead itself is therefore **not eliminated**. It is packaged as a synchronization transition rather than exposed as a misleading immediately-controllable replay state.

Post-release hardening should treat presentation as a slave clock while Direct Control owns simulation authority: suppress replay seek/rewind/highlight jumps, continuously detect presentation/live divergence, snap presentation back to live when necessary, and restore ordinary replay freedom only after confirmed `Ctrl+End` release.


## Cursor/world marker origin offset

Status: **known, non-blocking for coarse movement; blocking for precision clicking**.

The yellow world-space marker is consistently displaced from the physical mouse reticle. The displacement remains approximately constant across zoom levels, which strongly suggests that the scale is correct and the reference origin/viewport center is wrong.

Current diagnostics report `origin center_log`; the transform still uses `ingame.center_log` as a calibration reference. That node is not guaranteed to coincide with the real `Game` draw-map origin.

This must be corrected before precise actor/entity hit testing is considered production-ready. It does not invalidate the already-proven RMB MoveTo command path.

## Playback-speed controls during live pacing

Status: **post-release experiment; intentionally not in the first public release**.

While Candidate A is held near 60 Hz, the game's normal 0.5x/1x/1.5x/2x/3x replay controls do not meaningfully change live presentation speed. A release-week synchronization experiment did not produce a control model worth shipping: ordinary speeds remained unreliable, while Highlight could pause on champion death and then accelerate toward respawn when selected.

The first Workshop release therefore preserves the proven 1x/60 Hz pacing baseline. Synchronized ordinary speeds and controlled-champion death fast-forward are shelved for post-release testing rather than risking a less predictable relationship between presentation and simulation authority.

If speed-changing features return later, they must remain completely disabled in multiplayer; Direct Control's multiplayer speed policy is fixed 1x.

## Persistent MoveTo state

Status: **diagnostic behavior retained intentionally for now**.

The current movement command is persistent. A selected champion continues trying to reach the stored destination until another command replaces it. Runtime testing showed that this can survive pause/resume and even death/respawn.

That persistence is useful evidence that the command bridge is stable, but final MOBA-like behavior will probably clear or supersede movement on events such as arrival, death/respawn, attack orders, casts, or explicit stop/idle commands.

## Entity clickability branch

`feat/cursor-entity-picking` was created before the current athlete-aware selection and pacing architecture matured. Its entity-picker concepts remain useful, but the branch should **not** be merged wholesale.

When actor/entity targeting becomes the current task, transplant or rebase the picker logic onto the current live-control architecture. Expect manual integration in at least `src/lib.rs` and `src/control.rs`.
