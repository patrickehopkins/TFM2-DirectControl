# Camera controls

## Validated behavior

Middle-mouse drag is **physically validated** and matches the intended MOBA-style grab-and-drag behavior. Harbinger calculates a desired camera center from total cursor displacement, but moves the camera only through TFM2's native pan inputs; the game's real camera remains authoritative. This eliminated the earlier flicker, snap-back, and screen-to-world disagreement caused by directly overwriting derived camera-center fields.

MMB drag itself is considered validated. Preserve its position-servo/native-pan mechanics unless a reproducible regression requires reopening them.

Minimap camera relocation remains native TFM2 behavior and composes correctly with Direct Control's contextual RMB minimap commands.

## Match-wide availability

MMB drag and edge scrolling are **match-view QoL**, not champion-ownership features. They remain available throughout an interactive match whether the user is manually controlling a champion or spectating after `End` releases control.

Combat/control keybind mode may still switch between Direct Control and spectator behavior, but camera gestures must not disappear merely because no champion is selected.

## UI interaction during MMB

Physical testing found one remaining integration problem in the otherwise-good MMB path: crossing interactive UI can temporarily stall camera integration, and the minimap can slow the drag while the cursor is over it. The desired end result is still that MMB behaves as a pure viewport-map drag rather than an interaction with match UI.

A first attempt tried to fake a harmless native pointer location while MMB was held. That was **physically rejected**: it made MMB stutter and caused visible UI flicker. Do not reintroduce synthetic pointer movement, pointer-coordinate rewriting, or broad LMB/RMB suppression as an MMB fix.

Current retry keeps the known-good physical cursor polling untouched. Harbinger publishes only the desired native pan values; the native camera detour now applies those values synchronously immediately before TFM2's own camera handler runs. This is intended to make UI hover irrelevant to the pan-field race without lying to the game's pointer system.

If UI hover still stalls MMB after this pass, prefer restoring the last fully smooth MMB behavior rather than layering additional pointer-routing hacks onto it. A later deeper native UI/camera ownership investigation would then be the correct route.

## Edge-scroll completion

Edge scrolling is **not deferred**. It remains part of the current pre-release camera milestone and should be finished before moving on.

The confirmed failure pattern is highly specific:

- stationary physical edge hover advances in small/ticking increments;
- moving/wiggling the mouse while still touching the same edge produces smooth camera motion;
- top/bottom UI regions can suppress edge movement even though the OS cursor is physically at the real client edge;
- full-view and Info/split-view edge geometry can therefore appear different because different UI occupies those areas.

The synthetic-mouse-movement experiment was physically rejected and removed. The current approach instead eliminates the asynchronous pan-field race: the camera driver publishes the requested edge-pan vector, and the already-existing native camera hook writes that vector synchronously at camera-handler entry. No fake mouse messages are generated and no UI hover state is modified.

Required physical result:

- all four physical client edges work in both full and Info/split layouts;
- a completely stationary cursor glides continuously rather than ticking;
- top/bottom bars do not create dead camera zones;
- edge speed remains useful and consistent;
- no camera-center writes, synthetic pointer events, or second camera authority are introduced.

## Follow/recenter

A dedicated hold-to-center/follow action (for example Space) remains desirable after MMB and edge scrolling are both fully validated. Persistent follow/lock already exists natively through TFM2's F-key camera behavior and can later be exposed more cleanly through the custom shortcut pass.
