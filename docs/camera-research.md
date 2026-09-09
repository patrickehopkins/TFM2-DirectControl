# Camera / Screen-to-World Research

Status: active reverse-engineering notes for Teamfight Manager 2 v0.5.8.

This document records the evidence behind the camera adapter used by TFM2 Direct Control. Keep version-specific reverse-engineering details isolated here so the gameplay/control code can remain on the official stable mod API.

## Target build

Observed executable:

- `TeamfightManager2.exe`
- size: `77,666,816` bytes
- SHA-256: `4ed3aed08971efd06b7415817c9da9a444b6c7f63dc2ec09540b572568a4e045`
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

The camera input/update path was traced far enough to identify the following fields in the live match-camera-containing object.

### Confirmed field map

| Object offset | Meaning | Evidence |
| --- | --- | --- |
| `+0xE0` | camera zoom | Zoom-in/out changes this float by `+/-0.25`; value is clamped to `0.5 .. 3.0`. |
| `+0xE4` | camera center X | Used as visible-region center; whole-map camera initialization writes `480.0`. |
| `+0xE8` | camera center Y | Used as visible-region center; whole-map camera initialization writes `480.0`. |
| `+0x418` | horizontal pan velocity / input delta | Pan-right writes `+100.0`; pan-left writes `-100.0`. |
| `+0x41C` | vertical pan velocity / input delta | Pan-up writes `-100.0`; pan-down writes `+100.0`. |

The camera input handler was identified at RVA approximately `0x009E6750` in the tested executable.

### Coordinate scale

The match camera operates in a logical world space of approximately `960 x 960`, while the stable simulation uses `960000 x 960000` integer world coordinates. The observed conversion is:

```text
camera/logical world = simulation world * 0.001
simulation world      = camera/logical world * 1000
```

This is consistent with whole-map camera center `(480.0, 480.0)`.

### Additional camera-adjacent fields

Offsets `+0xEC` and `+0xF0` are read by visible-region/culling math and are divided by the current zoom. They are likely viewport/world-extent values, but their exact semantics are not yet proven. Do not use them as full width, height, or half-extents until verified.

## Current blocker

The location of the camera state inside the object is known. The remaining blocker is obtaining the current object's address safely from the stable mod.

Preferred approaches, in order:

1. Continue tracing ownership/callers until a stable owner/global path to the camera-containing object is found.
2. If no clean owner path emerges quickly, install a tiny version-checked hook at the confirmed camera handler and capture its `this` pointer when TFM2 invokes it.
3. Read only the confirmed camera fields from that captured object.

The adapter must verify the executable version/hash before using hard-coded RVAs/offsets. On mismatch it should disable camera readback rather than guess.

## Planned validation milestone

Before RMB movement is enabled, expose a diagnostic overlay showing at least:

```text
Camera center: (X, Y)
Zoom: Z
Mouse UI: (x, y)
Mouse world: (world_x, world_y)
```

Then verify all of the following while the match camera is unlocked:

- pan horizontally and vertically;
- zoom through several levels;
- move between full-screen and windowed mode;
- show/hide match-information panels;
- confirm the calculated world point remains under the cursor.

Only after the screen-to-world transform survives those tests should RMB issue `InputV1::move_to(...)`.

## Design boundary

The long-term design remains:

```text
Win32 mouse -> logical UI position -> battlefield viewport -> camera adapter -> world position -> stable Player AI InputV1
```

Keep reverse-engineered camera access in a narrow module. Everything above it should consume a small interface such as `CameraSnapshot { center_x, center_y, zoom, ... }` and remain independent of TFM2's private object layout.
