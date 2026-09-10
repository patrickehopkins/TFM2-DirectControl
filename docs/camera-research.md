# Camera / Screen-to-World Research

Status: active reverse-engineering notes for Teamfight Manager 2 v0.5.8.

This document records the evidence behind the camera adapter used by TFM2 Direct Control. Keep version-specific reverse-engineering details isolated here so the gameplay/control code can remain on the official stable mod API.

## Target build

Observed executable:

- `TeamfightManager2.exe`
- size: `77,666,816` bytes
- SHA-256: `4ed3aed08971efd06b7415817c9da9a444b6c7f63dc2ec09540b572568a4e045`
- PE timestamp: `0x6A978218`
- image size: `0x04A1D000`
- executable contains a `game.pdb` reference and retained Rust source-path / string information.

Treat all offsets and RVAs in this document as valid only for this exact executable until re-verified.

## Stable API findings

The stable client API is useful for the outer parts of the transform, but does not expose the live spectator-camera transform directly.

Confirmed:

- TFM2 reports a logical UI render space of `1920 x 1080` even when the physical client is `2560 x 1440`.
- The existing Win32 mouse probe correctly converts physical client coordinates into this logical UI space.
- The live UI tree exposes spectator-layout rectangles, buttons, panels, and other UI geometry.
- Moving the camera while keeping the same UI layout does not reveal camera center or zoom in `ui_state_json`, node rectangles, or other stable UI-tree data.
- Camera/zoom buttons appear in the tree, but their state payloads are empty.
- `ingame.center_log` tracks the visible battlefield rectangle. Observed layouts include approximately `(0,50,1920,974)` in the wide view and `(18,64,960,960)` with match information displayed.

Conclusion: stable UI is suitable for identifying the battlefield viewport rectangle, but not the current camera center/zoom.

## Classic native API route

The public classic 0.5 documentation describes a native extension API that receives internal `Scene` / `RenderState` values, but Patrick's installed v0.5.8 game directory contains only:

- `config`
- `mod-sdk-stable`
- `mods`

There is no `mod-sdk` directory and no `build_mod.bat` anywhere under the game directory. The classic-probe experiment was therefore abandoned and its temporary source/installer removed from the branch.

## Static executable string findings

A read-only static scan of the executable found the exact internal camera/input action registry, including:

- `in_game_pan_right`
- `in_game_pan_left`
- `in_game_pan_up`
- `in_game_pan_down`
- `in_game_zoom_in`
- `in_game_zoom_out`
- `in_game_camera_all`
- `in_game_camera_team0`
- `in_game_camera_team1`
- `in_game_auto_follow`
- all ten own/enemy player-follow actions

Other relevant retained strings include client-view origins such as `ClientMatchView`, `ClientSpectate`, and `ClientReplay`.

No trustworthy game-camera symbol named `screen_to_world`, `world_to_screen`, `camera_position`, `camera_zoom`, or equivalent was found. A `worldToCamera/worldToNDC` string hit belongs to the bundled EXR image library and is unrelated to TFM2's match camera.

## Disassembly findings for v0.5.8

### Confirmed field map

| Object offset | Meaning | Evidence |
| --- | --- | --- |
| `+0xE0` | camera zoom | Zoom-in/out changes this float by `+/-0.25`; value is clamped to `0.5 .. 3.0`. Runtime screenshots confirmed 0.50, 1.00, and 3.00. |
| `+0xE4` | camera center X | Used as visible-region center; whole-map camera initialization writes `480.0`. Runtime value follows horizontal free-camera pan. |
| `+0xE8` | camera center Y | Used as visible-region center; whole-map camera initialization writes `480.0`. Runtime value follows vertical free-camera pan. |
| `+0xEC` | current square render-world extent X | Initialized from `1024.0` and divided by zoom. Runtime values confirm `1024 / zoom`. |
| `+0xF0` | current square render-world extent Y | Initialized from `1024.0` and divided by zoom. Runtime values confirm `1024 / zoom`. |
| `+0xF4` | camera mode byte | Stayed `0` throughout the tested free-camera session. Exact nonzero-mode meanings remain unclassified. |
| `+0x418` | horizontal pan velocity / input delta | Pan-right writes `+100.0`; pan-left writes `-100.0`. |
| `+0x41C` | vertical pan velocity / input delta | Pan-up writes `-100.0`; pan-down writes `+100.0`. |

The camera input handler starts at RVA `0x009E6750` in the tested executable. Its first 12 bytes are eight complete push instructions:

```text
55 41 57 41 56 41 55 41 54 56 57 53
```

That makes the entry suitable for a small trampoline without relocating RIP-relative instructions.

### Ownership / embedding trace

Static caller tracing produced an additional structural check:

```text
outer object
  +0x4A70 -> scene / active match-view object
                 +0x960 -> primary camera-containing subobject
```

The large client update path passes `scene + 0x960` as the first argument (`this`) into the camera handler wrapper. A second camera-like subobject is also routed through the same generic handler on another path, so runtime capture keeps multiple candidates instead of assuming the first/last call is always the active spectator camera.

This ownership trace is useful for validating captured pointers even though the stable API does not expose the `outer` object directly.

### Coordinate scale

The match camera operates in a logical world space of approximately `960 x 960`, while the stable simulation uses `960000 x 960000` integer world coordinates. The observed conversion is:

```text
camera/logical world = simulation world * 0.001
simulation world      = camera/logical world * 1000
```

This is consistent with whole-map camera center `(480.0, 480.0)`.

### Extent behavior

Static disassembly and runtime observation now agree on the meaning of `+0xEC/+0xF0`.

The camera update path initializes both axes from `(1024.0, 1024.0)` and divides that vector by the current zoom before storing it. Runtime captures showed:

| Zoom | Extent X | Extent Y |
| ---: | ---: | ---: |
| `0.50` | `2048.00` | `2048.00` |
| `1.00` | `1024.00` | `1024.00` |
| `3.00` | `341.33` | `341.33` |

Therefore:

```text
extent_x = extent_y = 1024 / zoom
```

These values describe the square 2048x2048 Game render target's world extent. The visible battlefield UI is a centered crop of that square target, with its crop rectangle supplied by `ingame.center_log`.

## Runtime capture validation

The feature branch contains `src/camera_probe.rs`, a deliberately narrow v0.5.8 adapter.

Safety/containment rules:

- verifies the main module's PE timestamp and image size;
- verifies the exact 12-byte camera-handler prologue before patching;
- refuses to install on a mismatched build;
- copies only whole non-RIP-relative prologue instructions into an executable trampoline;
- detours only the confirmed camera handler;
- calls the original handler normally, then snapshots the verified fields;
- publishes floats through atomics so the stable UI overlay never dereferences stale camera pointers;
- keeps up to four candidate camera objects because the handler is shared by more than one camera-like subobject;
- does not issue movement or alter gameplay state.

The first live test succeeded without a crash. During the session:

- exactly one camera candidate appeared;
- its address stayed stable for the session (`0x0000027823BDE348` in that run; the absolute address is process-specific and must never be hard-coded);
- camera center followed free pan across the map;
- zoom followed the game's controls at `0.50`, `1.00`, and `3.00`;
- extents followed `1024 / zoom` exactly to displayed precision;
- mode remained `0` during free-camera use;
- handler call count increased continuously, confirming the captured object is live rather than stale state.

Representative captures included centers `(732.75,234.75)`, `(334.50,381.00)`, `(573.75,855.75)`, `(524.25,156.00)`, `(166.50,120.00)`, `(840.00,932.25)`, and `(120.00,7.50)`.

Conclusion: camera-object acquisition, center readback, zoom readback, and square render extent are all validated for the target v0.5.8 executable.

## Proposed screen-to-world projection

Stable drawing reports the `Game` render map as `2048 x 2048`. With camera extent `1024 / zoom`, each Game-render pixel covers:

```text
logical world units per Game pixel = extent / 2048
                                   = 0.5 / zoom
```

Let the live `ingame.center_log` rectangle be `(vx, vy, vw, vh)` and its center be `(vcx, vcy)`. If the UI battlefield is the centered crop observed in the screenshots, then a logical UI mouse point maps into Game-render coordinates as:

```text
game_x = game_width  / 2 + (mouse_ui_x - vcx)
game_y = game_height / 2 + (mouse_ui_y - vcy)
```

and into camera/logical world coordinates as:

```text
world_x = camera_center_x + (game_x - game_width/2)  * extent_x / game_width
world_y = camera_center_y + (game_y - game_height/2) * extent_y / game_height
```

For the observed 2048x2048 target this simplifies to:

```text
world_x = camera_center_x + (mouse_ui_x - vcx) * (0.5 / zoom)
world_y = camera_center_y + (mouse_ui_y - vcy) * (0.5 / zoom)

sim_x = world_x * 1000
sim_y = world_y * 1000
```

Y is expected to increase downward, consistent with both screen coordinates and the observed positive pan-down direction.

This projection is strongly supported but is not yet considered final until a direct composition test confirms the Game-space marker remains under the UI-space cursor across layouts, pan, zoom, fullscreen, and windowed mode.

## Current validation milestone

Before RMB movement is enabled, the diagnostic build should:

1. read the live `ingame.center_log` viewport;
2. map the mouse to the corresponding centered point in the 2048x2048 `Game` render map;
3. draw a distinctive Game-space marker there while retaining the existing UI-space cursor crosshair;
4. display calculated logical-world and simulation-world coordinates.

The two markers should overlap throughout:

- horizontal and vertical pan;
- several zoom levels;
- wide and Match Info layouts;
- fullscreen and windowed mode.

Only after this composition/projection test passes should RMB issue `InputV1::move_to(...)`.

## Design boundary

The long-term design remains:

```text
Win32 mouse -> logical UI position -> battlefield viewport -> camera adapter -> world position -> stable Player AI InputV1
```

Keep reverse-engineered camera access in a narrow module. Everything above it should consume a small interface such as `CameraSnapshot { center_x, center_y, zoom, extent_x, extent_y }` and remain independent of TFM2's private object layout.
