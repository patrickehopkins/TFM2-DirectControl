# Camera controls

## Validated behavior

Middle-mouse drag is now **physically validated** and matches the intended MOBA-style grab-and-drag behavior. Harbinger calculates a desired camera center from total cursor displacement, but moves the camera only through TFM2's native pan inputs; the game's real camera remains authoritative. This eliminated the previous flicker, snap-back, and screen-to-world disagreement caused by directly overwriting derived camera-center fields.

MMB drag is considered the primary free-camera control and should not be reopened absent a reproducible regression.

Minimap camera relocation remains native TFM2 behavior and composes correctly with Direct Control's contextual RMB minimap commands.

## Match-wide availability

MMB drag and edge scrolling are **match-view QoL**, not champion-ownership features. They should remain available throughout an interactive match whether the user is manually controlling a champion or spectating after `End` releases control.

Combat/control keybind mode may still switch between Direct Control and spectator behavior, but camera gestures must not disappear merely because no champion is selected.

## Remaining edge-scroll polish

Edge scrolling is still imperfect:

- stationary edge hover advances in small/ticking increments rather than the desired smooth glide;
- moving the mouse while touching the edge produces much smoother native movement;
- top/bottom activation can be blocked or interrupted by UI regions such as the team stats/control bars;
- full-view and Info/split-view edge geometry do not behave identically.

MMB is now strong enough that edge-scroll polish should not be allowed to destabilize the validated camera path. Keep edge scrolling as a pre-release polish candidate, but it may be deprioritized if fixing the UI/cadence interaction becomes disproportionately invasive.

## Follow/recenter

A dedicated hold-to-center/follow action (for example Space) remains desirable once the core camera path is stable. Persistent follow/lock already exists natively through TFM2's F-key camera behavior and can later be exposed more cleanly through the custom shortcut pass.
