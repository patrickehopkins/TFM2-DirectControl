# Direct-Control Validation Log

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

The purpose is to isolate the on-screen simulation from the millions of unrelated `Unknown` callbacks without globally injecting mouse-driven input into every predictive simulation.
