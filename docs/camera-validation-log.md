# Camera Projection Validation Log

This log records physical tests of the v0.5.8 camera adapter and screen-to-world projection. The durable reverse-engineering summary remains in `docs/camera-research.md`.

## 2026-09-09 - Live camera capture validation

Runtime hook succeeded without a crash. Exactly one camera candidate appeared during the tested match and its process-specific address stayed stable. Camera center followed free pan; zoom tracked the game's controls; extent followed `1024 / zoom` exactly.

Observed examples:

- zoom `0.50` -> extent `(2048.00, 2048.00)`
- zoom `1.00` -> extent `(1024.00, 1024.00)`
- zoom `1.75` -> extent `(585.14, 585.14)`
- zoom `3.00` -> extent `(341.33, 341.33)`

The test also confirmed that `ingame.center_log` changes with UI layout while the camera readback remains valid.

## 2026-09-09 - Projection validation v1

The first composition test converted the UI cursor displacement from the center of `ingame.center_log` into a putative raw 2048x2048 Game-render coordinate, then called `draw_circle("Game", game_x, game_y, ...)`.

Result: **incorrect except at fully zoomed-out `0.50x`**. The behavior was consistent in both fullscreen/wide and Match Info layouts.

This failure is informative. The stable API defines the `"Game"` draw map as **match-world space**, not raw 2048x2048 render-target pixel space. The v1 test accidentally treated `draw_map_size("Game") == 2048x2048` as the coordinate system accepted by Game drawing.

Why `0.50x` appeared correct:

```text
live camera extent at 0.50x = 2048 x 2048
stable Game backing map      = 2048 x 2048
```

At that zoom only, the incorrect raw-pixel assumption happens to match the live camera extent numerically. At other zoom levels the game's live camera changes extent while the mod's Game drawing camera remains at its default, so the yellow marker drifts away from the UI crosshair.

This result does **not** invalidate the captured camera center, zoom, extent, or the UI-to-world formula. It invalidates only the way the diagnostic marker was rendered.

## 2026-09-09 - Projection validation v2

The corrected test explicitly matches the stable Game draw camera to the captured TFM2 camera:

```text
draw_set_camera("Game", center_x, center_y, extent_x, extent_y)
```

The calculated logical-world coordinate remains:

```text
dx = mouse_ui_x - viewport_center_x
dy = mouse_ui_y - viewport_center_y

world_x = camera_center_x + dx * extent_x / 2048
world_y = camera_center_y + dy * extent_y / 2048

sim_x = world_x * 1000
sim_y = world_y * 1000
```

After setting the Game drawing camera, the diagnostic draws the yellow rings directly at `(world_x, world_y)` in match-world space.

Result: **PASS**.

Physical testing confirmed that the yellow world-space marker stayed centered on the white UI cursor crosshair while:

- panning freely across the map;
- changing zoom, including `0.50x`, `1.00x`, `2.50x`, and `3.00x` captures;
- using the wide/fullscreen battlefield layout;
- displaying Match Info;
- moving the cursor around the visible battlefield.

The off-viewport behavior also passed. When the cursor moved outside `ingame.center_log`, the overlay reported `cursor inside no` and stopped producing/drawing a world target. This prevents clicks on scoreboard, side panels, and other UI from being converted into battlefield commands.

Conclusion: the v0.5.8 physical mouse -> logical UI -> live battlefield viewport -> captured camera -> simulation-world projection is validated and is ready to feed the first real movement-control test.
