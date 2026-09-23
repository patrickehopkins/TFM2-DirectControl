# Camera controls

## Status — locked / validated

Camera controls are considered **done for now**. Do not alter the validated MMB or wheel behavior during unrelated work. Reopen this area only for the planned resolution/UI-scaling compatibility audit or for a reproducible regression.

Physically validated behavior:

- **MMB grab-and-drag — PASS.** The camera follows total physical cursor displacement at the desired 1:1 feel without a speed ceiling, catch-up teleport, flicker, snap-back, or screen-to-world disagreement.
- **MMB over native UI — PASS.** The drag continues normally across the minimap, player cards, buttons, and top/bottom match UI. Those UI regions no longer stall or slow the camera while MMB is held.
- **Mouse-wheel zoom — PASS.** Wheel up zooms in and wheel down zooms out using the native-sized `0.25` step and validated `0.5 .. 3.0` range.
- **Match-wide availability — PASS.** MMB drag and wheel zoom remain available after `End` releases the controlled champion and returns the user to ordinary spectator/AI control.
- Native minimap LMB camera relocation continues to compose with Direct Control's contextual RMB minimap commands.

## Architecture to preserve

Harbinger never writes the derived camera center at `+0xE4/+0xE8`. Earlier direct-center ownership caused flicker, snap-back, and disagreement between the rendered camera and RMB world projection.

MMB instead computes a desired center from total physical cursor displacement and publishes native pan requests. The version-checked camera hook applies those requests synchronously immediately before TFM2's own camera handler runs, leaving TFM2 authoritative for camera-center integration, bounds, follow state, minimap relocation, and rendering.

While MMB is held, a narrow window-procedure shim prevents native match UI from capturing the gesture. Harbinger still reads the true OS cursor directly for camera displacement. Real incoming mouse-move events are presented to native UI at an inert battlefield coordinate, and native MMB/UI click handling is suppressed for the duration of the hold. No synthetic mouse messages are generated and the OS cursor is never moved. Releasing MMB immediately restores ordinary native UI interaction.

Wheel input is queued to the active camera and applied synchronously by the same native camera hook. Wheel zoom is ignored during an active MMB hold so the captured drag scale cannot change underneath its anchor.

These mechanics are now validated behavior. Preserve them unless the later scaling audit proves that one of their coordinate assumptions is wrong.

## Screen-edge scrolling — shelved

Custom screen-edge scrolling is **shelved by design** and is not part of the current release-critical camera behavior. All Harbinger edge detection/pan behavior has been removed from the active camera driver.

The unresolved behavior is retained only as investigation history: stationary edge hover ticked rather than gliding smoothly, and top/bottom UI regions suppressed edge movement. Physical mouse motion while touching an edge made the native path smoother, but manufacturing that cadence with synthetic mouse messages created visible UI flicker and was rejected.

If edge scrolling is ever revisited, investigate the native camera/update ownership path directly. Do not restore synthetic mouse wakes, direct camera-center ownership, or other workarounds that disturb native pointer/UI state.

## Resolution / UI-scaling audit

Current physical validation has been performed on one native-resolution setup. A dedicated pre-release compatibility pass must verify that resolution, window size, aspect ratio, Windows DPI/display scaling, or any in-game UI-scale option does not invalidate coordinate assumptions.

Most gameplay targeting already converts the physical cursor through live `draw_map_size("UI")`, live `ingame.center_log`, and live minimap rectangles. MMB is the highest-risk path because its camera driver currently converts physical cursor displacement using the validated `1920 x 1080` logical-UI assumption outside `StableClient`.

The compatibility audit should cover multiple 16:9 resolutions/window sizes, both full and Info/split match layouts, any available in-game UI-scale setting, and a non-default Windows display scale if practical. Re-test RMB projection, skill aim, minimap input, MMB 1:1 drag, wheel zoom, HUD/click geometry, and UI bypass behavior during MMB. If MMB scaling fails, publish live UI/battlefield geometry from the stable client to the camera adapter rather than adding resolution-specific constants.

## Later optional camera polish

- **Screen-edge scrolling:** shelved until/unless a deeper native-camera route is worth revisiting.
- **Space recenter/follow:** promoted back into the first-release polish pass. Two custom pan-chase implementations were physically rejected because they jerked/overshot while following a moving simulation target. Two attempts to synthesize the native F-key follow input (posted window messages, then Windows keyboard injection) were also physically rejected because TFM2 did not respond. The release branch has been restored to the validated camera baseline while a targeted v0.6.1 native-follow action probe traces `in_game_follow_own_*`, `in_game_follow_enemy_*`, and `in_game_auto_follow` so Space can drive the real follow dispatcher/state directly. Physical validation pending.
