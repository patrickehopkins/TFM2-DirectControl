# Camera controls

## Validated behavior

Middle-mouse drag is **physically validated** and matches the intended MOBA-style grab-and-drag behavior. Harbinger calculates a desired camera center from total cursor displacement, but moves the camera only through TFM2's native pan inputs; the game's real camera remains authoritative. This eliminated the earlier flicker, snap-back, and screen-to-world disagreement caused by directly overwriting derived camera-center fields.

MMB drag itself is considered validated. Preserve its position-servo/native-pan mechanics unless a reproducible regression requires reopening them.

Minimap camera relocation remains native TFM2 behavior and composes correctly with Direct Control's contextual RMB minimap commands.

## Match-wide availability

MMB drag and mouse-wheel zoom are **match-view QoL**, not champion-ownership features. They should remain available throughout an interactive match whether the user is manually controlling a champion or spectating after `End` releases control.

Combat/control keybind mode may still switch between Direct Control and spectator behavior, but these camera gestures must not disappear merely because no champion is selected.

## UI interaction during MMB

Physical testing narrowed the remaining MMB integration problem substantially. The position-servo behavior itself is correct, and MMB now moves normally across the minimap, but native UI regions such as player cards, buttons, and the top/bottom match bars can still stall camera integration while the physical cursor crosses them.

A previous attempt used synthetic mouse movement plus broad pointer-coordinate spoofing. That was **physically rejected**: it made MMB stutter and caused visible UI flicker. Synthetic mouse messages, cursor movement, and periodic fake edge wakes are prohibited going forward.

The current pass is the final narrow retry before accepting the known-good MMB behavior as-is. While MMB is held:

- Harbinger still reads the true OS cursor directly for the actual drag displacement;
- native MMB press/release is swallowed because MMB has no desired native match action;
- each **real** incoming mouse-move event is forwarded to native UI at a stable inert battlefield coordinate, alternating by only one physical pixel so the native update path cannot collapse identical events;
- no extra mouse-move messages are generated;
- native LMB/RMB UI activation is suppressed during the MMB hold, while Direct Control can still observe the physical buttons through `GetAsyncKeyState` for gameplay input;
- releasing MMB immediately returns pointer ownership to the native UI, with no synthetic hover-restoration message.

The intended result is that MMB behaves as a pure viewport-map gesture: cards, buttons, bars, and the minimap cannot capture or slow it. If this pass causes stutter, visible UI flicker, or any other regression, revert the UI-routing shim and keep the previously validated MMB behavior rather than adding another workaround.

## Mouse-wheel zoom

Mouse-wheel zoom is now part of the match-wide camera QoL test:

- wheel **up** requests one native-sized `+0.25` zoom step (zoom in);
- wheel **down** requests one `-0.25` step (zoom out);
- zoom remains clamped to the validated native `0.5 .. 3.0` range;
- wheel delta is accumulated in standard 120-unit notches so high-resolution wheels do not become excessively sensitive;
- wheel input is queued to the active camera and applied synchronously by the existing native camera hook rather than mutating camera state from the UI thread;
- wheel zoom is intentionally ignored during an active MMB hold so the drag's captured screen/world scale cannot change underneath its anchor.

## Screen-edge scrolling — shelved

Custom screen-edge scrolling is **shelved for now by design**. All current Harbinger edge detection/pan behavior has been removed from the active camera driver rather than leaving a half-working implementation in place.

The unresolved behavior is documented for a possible later revisit: stationary edge hover ticked instead of gliding smoothly, and top/bottom UI regions suppressed edge movement. Physical mouse motion while touching an edge made the native path smoother, but attempts to manufacture that cadence with synthetic mouse messages created visible UI flicker and were rejected.

If edge scrolling is revisited, investigate the native camera/update ownership path directly. Do not restore the synthetic-mouse wake approach merely because it produced continuous movement.

## Resolution / UI-scaling risk

Current physical validation has been performed at one native-resolution setup. A dedicated pre-release compatibility pass must verify that Direct Control is not accidentally dependent on that exact resolution, aspect ratio, Windows DPI scale, or in-game UI scaling.

Most gameplay targeting already converts the physical cursor through live `draw_map_size("UI")`, live `ingame.center_log`, and live minimap rectangles. MMB is the most obvious remaining risk because its camera driver currently converts physical cursor displacement using the validated `1920 x 1080` logical-UI assumption outside `StableClient`.

The later compatibility audit should cover at least multiple 16:9 resolutions/window sizes, both full and Info/split match layouts, any available in-game UI-scale setting, and a non-default Windows display scale if practical. Re-test RMB projection, skill aim, minimap input, MMB 1:1 drag, wheel zoom, and HUD/click geometry. If MMB scaling fails, publish the live UI/battlefield geometry from the stable client to the camera adapter rather than adding more resolution-specific constants.

## Follow/recenter

A dedicated hold-to-center/follow action (for example Space) remains desirable after the current MMB/wheel camera pass is stable. Persistent follow/lock already exists natively through TFM2's F-key camera behavior and can later be exposed more cleanly through the custom shortcut pass.
