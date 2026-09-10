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

### Origin-gate hypothesis

The first movement build only allowed `SimOriginKindV1::ClientMatchView`. The stable API also defines `ClientSpectate` as a distinct live-client simulation origin. TFM2's coach/spectator-style in-game presentation may invoke player-input AI under `ClientSpectate`, causing the original safety gate to reject every manual command and return vanilla input.

The next diagnostic build therefore:

- allows both `ClientMatchView` and `ClientSpectate`;
- continues rejecting server pre-sims, replays, tools, and unknown origins;
- records total AI `think()` calls;
- records selected-player callback hits;
- records manual move and manual-idle returns;
- records the last callback player ID and all player IDs observed;
- reports whether the selected callback is running under `ClientMatchView`, `ClientSpectate`, or another rejected origin.

This instrumentation is intended to distinguish an origin-gate problem from a player-ID mapping or input-application problem without changing the validated camera/projection path.
