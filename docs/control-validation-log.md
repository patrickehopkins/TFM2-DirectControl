# Direct-Control Validation Log

> Historical note: this file preserves the early control-path investigation. Several hypotheses below were later superseded by Candidate-A pacing work. In particular, the `match_view + 0x250/+0x258` playback-field interpretations were physically rejected, while the actual watched simulation was identified as Candidate A and successfully paced near 60 Hz. See `docs/pacing-validation-log.md` and `docs/manual-control-validation.md` for the current validated architecture.

This log records physical tests of the manual-control bridge. Camera reverse-engineering and projection validation are documented separately in `docs/camera-research.md` and `docs/camera-validation-log.md`.

## 2026-09-09 - F-key selection and RMB publication

First live control test:

- F1-F10 selected the expected ten visible champions in order.
- RMB battlefield clicks were detected.
- projected simulation coordinates were in bounds.
- the cyan command marker appeared at the clicked world point.
- `last move target` changed after RMB clicks.
- selected champions did **not** obey the movement commands.

Conclusion: mouse acquisition, viewport gating, camera projection, F-key selection UI, and client-side command publication all worked. The failure was downstream of publication, at or inside the `StablePlayerAi::think()` override path.

## Origin diagnostics

### ClientMatchView-only gate

The first movement build only allowed `SimOriginKindV1::ClientMatchView`. The selected-player callback ran, but no live movement was returned after the match entered its normal running state.

### ClientMatchView + ClientSpectate gate

A second diagnostic build allowed both named live-client origins and counted every AI callback. Physical testing showed:

- all ten player IDs `[0,1,2,3,4,5,6,7,8,9]` appeared in `StablePlayerAi::think()`;
- `think` and `selected hits` increased extremely rapidly (millions of callbacks in seconds);
- `move returns` stayed at zero;
- the previously accumulated `idle returns` stopped increasing during normal live play;
- the dominant selected callback origin was reported as `other/rejected`.

This ruled out player-ID mapping and proved that the selected-player AI callback was running, while the origin filter was excluding the path used during the visible match.

### ServerPresim diagnostic gate

A temporary build also allowed `ServerPresim` and expanded exact origin classification. Physical testing showed:

- some manual-idle returns occurred through `ServerPresim`;
- those returns did not control the visible champion;
- the overwhelming majority of selected callbacks during the running match were explicitly `SimOriginKindV1::Unknown`;
- `move returns` still stayed at zero because `Unknown` remained blocked.

The callback volume is much larger than one 10-player, 60-Hz match could produce. Therefore `StablePlayerAi` is also being exercised inside predictive/background simulations. It is unsafe to simply accept every `Unknown` callback.

## 0.5.8 live-clock gate

The UI-tree investigation previously identified the visible match clock at:

`ingame.header.game_time.value`

with values such as `00:07`, `00:11`, `00:17`, etc. The stable simulation uses 60 ticks per second, and `StableAiContext` exposes the current simulation `tick`.

The next diagnostic/control build therefore uses a narrow 0.5.8 compatibility rule:

- the client render thread parses the visible match clock and publishes the current displayed match-second;
- `ClientMatchView` and `ClientSpectate` remain directly eligible;
- `ServerPresim`, replay, tool, and unrecognized origins are rejected;
- `Unknown` is accepted only when `ctx.tick() / 60` equals the visible UI match-second exactly;
- the overlay reports `unk@clock` match count, visible clock, and the most recent matching simulation tick.

The purpose was to isolate the on-screen simulation from the millions of unrelated `Unknown` callbacks without globally injecting mouse-driven input into every predictive simulation.

### Physical result

The clock gate worked exactly as a diagnostic filter but did not create interactive control:

- visible `00:11` matched approximately tick `719`, `00:16` matched tick `959`, and `00:22` matched tick `1379`, confirming the 60 Hz time relationship;
- `unk@clock` increased and `InputV1::move_to(...)` was actually returned on matching ticks;
- after one RMB destination was published, `move returns` increased continuously even without additional clicks. This is expected because movement commands are persistent and the hook returns the same target every accepted tick until another command replaces it;
- the watched champion still did not react to the returned movement input;
- the decisive observation was that all AI diagnostic counters eventually froze while the visible match clock continued advancing.

Conclusion: the `StablePlayerAi` callback stream being observed is decoupled from presentation time and runs ahead of the match being watched. Matching a callback tick to the current displayed second proves that the command reaches a corresponding simulation tick, but it does not make that callback an interactive presentation-time input point. The clock-gated `Unknown` path is diagnostic only and should not become the production architecture.

## Native architecture evidence after the clock-gated test

Static inspection of Teamfight Manager 2 v0.5.8 added two important findings.

### Server precomputed result versus watched/live result

The executable contains `GamePlayDone` diagnostics stating that:

- a precomputed server result can be overridden by a different live result;
- the `server/live simulation` can diverge;
- stored statistics and replay remain from the server run, so they can differ from what was watched.

This demonstrates that TFM2 distinguishes its server precomputed run from a separate result associated with the watched match. Therefore the failed stable-AI experiment does **not** imply that interactive direct control is impossible; it means we have not yet reached the local simulation path that feeds the watched result.

### Match-view playback-field hypothesis (later rejected)

Static inspection suggested candidate playback fields near `+0x250/+0x258`. Later runtime validation showed that `+0x250` remained constant and common interpretations of `+0x258/+0x260` did not track presentation. A wider match-view scan also failed to find a simple scalar presentation clock. These fields must not be used for pacing.

### Superseding result

Subsequent runtime work identified Candidate A as the watched-match simulation job and proved that pacing its confirmed worker at ~60 ticks per wall-clock second keeps the simulation live alongside presentation. Real-time manual `InputV1::move_to` injection, pause/resume, athlete-aware F1-F10 selection, and Ctrl+End release are now physically verified.

The current engineering target is therefore **command expansion**, not rediscovering the local-simulation/playback boundary.
