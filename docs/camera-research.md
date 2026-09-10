# Camera / Screen-to-World Research

Status: validated for Teamfight Manager 2 v0.5.8 target executable.

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
- The Win32 mouse probe correctly converts physical client coordinates into this logical UI space.
- The live UI tree exposes spectator-layout rectangles, buttons, panels, and other UI geometry.
- Moving the camera while keeping the same UI layout does not reveal camera center or zoom in `ui_state_json`, node rectangles, or other stable UI-tree data.
- Camera/zoom buttons appear in the tree, but their state payloads are empty.
- `ingame.center_log` tracks the visible battlefield rectangle. Observed layouts include approximately `(0,50,1920,974)` in the wide view and `(18,64,960,960)` with Match Info displayed.

Conclusion: stable UI is suitable for identifying the battlefield viewport rectangle, but not the current camera center/zoom.

## Classic native API route

The public classic 0.5 documentation describes a native extension API that receives internal `Scene` / `RenderState` values, but the installed v0.5.8 game directory contains only:

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
| `+0xE0` | camera zoom | Zoom-in/out changes this float by `+/-0.25`; value is clamped to `0.5 .. 3.0`. Runtime screenshots confirmed multiple zoom levels. |
| `+0xE4` | camera center X | Used as visible-region center; whole-map camera initialization writes `480.0`. Runtime value follows horizontal free-camera pan. |
| `+0xE8` | camera center Y | Used as visible-region center; whole-map camera initialization writes `480.0`. Runtime value follows vertical free-camera pan. |
| `+0xEC` | current square render-world extent X | Initialized from `1024.0` and divided by zoom. Runtime values confirm `1024 / zoom`. |
| `+0xF0` | current square render-world extent Y | Initialized from `1024.0` and divided by zoom. Runtime values confirm `1024 / zoom`. |
| `+0xF4` | camera mode byte | Stayed `0` throughout tested free-camera sessions. Exact nonzero-mode meanings remain unclassified. |
| `+0x418` | horizontal pan velocity / input delta | Pan-right writes `+100.0`; pan-left writes `-100.0`. |
| `+0x41C` | vertical pan velocity / input delta | Pan-up writes `-100.0`; pan-down writes `+100.0`. |

The camera input handler starts at RVA `0x009E6750` in the tested executable. Its first 12 bytes are eight complete push instructions:

```text
55 41 57 41 56 41 55 41 54 56 57 53
```

That makes the entry suitable for the small trampoline used by `src/camera_probe.rs` without relocating RIP-relative instructions.

### Ownership / embedding trace

Static caller tracing produced an additional structural check:

```text
outer object
  +0x4A70 -> scene / active match-view object
                 +0x960 -> primary camera-containing subobject
```

The large client update path passes `scene + 0x960` as the first argument (`this`) into the camera handler wrapper. Runtime capture has consistently produced one active camera candidate in tested live matches.

### Coordinate scale

The match camera operates in a logical world space of approximately `960 x 960`, while the stable simulation uses `960000 x 960000` integer world coordinates:

```text
camera/logical world = simulation world * 0.001
simulation world      = camera/logical world * 1000
```

This is consistent with whole-map camera center `(480.0, 480.0)`.

### Extent behavior

Static disassembly and runtime observation agree that:

```text
extent_x = extent_y = 1024 / zoom
```

Representative captures:

| Zoom | Extent X | Extent Y |
| ---: | ---: | ---: |
| `0.50` | `2048.00` | `2048.00` |
| `1.00` | `1024.00` | `1024.00` |
| `1.75` | `585.14` | `585.14` |
| `2.50` | `409.60` | `409.60` |
| `3.00` | `341.33` | `341.33` |

These values describe the square Game render-map world extent.

## Runtime camera capture

`src/camera_probe.rs` is a narrow v0.5.8 adapter. It:

- verifies the main module's PE timestamp and image size;
- verifies the exact 12-byte camera-handler prologue before patching;
- refuses to install on a mismatched build;
- copies only whole non-RIP-relative prologue instructions into an executable trampoline;
- detours only the confirmed camera handler;
- calls the original handler normally, then snapshots the verified fields;
- publishes floats through atomics so higher-level code never dereferences stale camera pointers;
- does not directly mutate gameplay state.

Live testing succeeded without crashes. Camera center followed pan, zoom followed game controls, extents followed `1024 / zoom`, and the handler call count increased continuously.

## Validated screen-to-world projection

The stable API reports `Game` as match-world space and its backing map size as `2048 x 2048`. Let the current `ingame.center_log` rectangle be `(vx, vy, vw, vh)` and its center `(vcx, vcy)`.

The validated mapping is:

```text
dx = mouse_ui_x - vcx
dy = mouse_ui_y - vcy

world_x = camera_center_x + dx * extent_x / 2048
world_y = camera_center_y + dy * extent_y / 2048

sim_x = world_x * 1000
sim_y = world_y * 1000
```

Y increases downward, consistent with both UI coordinates and camera pan behavior.

A first marker test failed because it incorrectly treated the stable `Game` map as raw 2048x2048 screen pixels. That happened to align only at `0.50x`, where the live camera extent is also 2048. The corrected test explicitly set the mod draw camera to the captured live camera:

```text
draw_set_camera("Game", center_x, center_y, extent_x, extent_y)
```

and drew the marker directly at the calculated world coordinate.

### Physical validation result

**PASS.** The world-space marker remained centered on the UI cursor while:

- freely panning;
- testing multiple zoom levels from `0.50x` through `3.00x`;
- using the wide/fullscreen battlefield layout;
- displaying Match Info.

Off-viewport behavior also passed: when the cursor is outside `ingame.center_log`, no battlefield world target is produced. This prevents ordinary UI interaction from becoming a movement command.

The complete validated pipeline is therefore:

```text
Win32 physical mouse
    -> logical 1920x1080 UI coordinate
    -> ingame.center_log battlefield crop
    -> captured v0.5.8 camera center/extent
    -> logical world coordinate
    -> x1000
    -> stable simulation coordinate
```

See `docs/camera-validation-log.md` for the chronological physical-test record.

## Current implementation boundary

The branch can now use the validated simulation-world point as input to `StablePlayerAi`. Reverse-engineered camera access remains isolated in `src/camera_probe.rs`; gameplay control code should consume only snapshots/derived world coordinates.

The first control slice is deliberately limited to player selection plus RMB movement. Attack targeting, skills, recall, and other controls remain separate later milestones.
