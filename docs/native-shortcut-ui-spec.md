# Harbinger native shortcut menu — audited scope and implementation contract

Status: **current design / source audit, not implemented or physically validated**.

External comparison note: Control v0.9 has a polished custom Ctrl+K key/settings overlay and a centralized JSON-backed action registry. That validates the need for a central binding layer, but it does **not** solve the native TFM2 Shortcuts integration/compatibility goal defined here. Based on `main` Harbinger v0.1.5 for Teamfight Manager 2 v0.6.2, audited October 4, 2026. This document supersedes the broad proposal portions of `docs/keybind-plan.md` for shortcut work. Do not change the established 60 Hz authoritative simulation or merge functional changes without maintainer approval.

## Objective

Put a configurable Harbinger Direct Control section alongside the game's normal shortcut settings, preferably using **the game's existing UI assets and native widget templates with no custom styling**. If the mod menu exposes a supported settings entry point, link to the same native section rather than maintaining two configurations. Respect other UI mods by using the existing layout and shared assets, avoiding hard-coded colors, font choices, overlays, or asset overrides. Native template availability and actual settings/mod-menu parent paths must be established on v0.6.2 before UI injection.

Harbinger bindings temporarily win over genuinely conflicting **native shortcut actions** in the appropriate Direct Control context. Otherwise the native bindings remain untouched. Never rewrite the player's stored shortcuts as a way of managing conflicts; restore native behavior when Harbinger is globally released or the session ends. Unsafe replay actions are the exception: they stay semantically disabled throughout match ownership, including temporary spectator yield.

## Existing controls — source-audited

| Harbinger action | Current default | Input path | Enabled when |
| --- | --- | --- | --- |
| Start Direct Control | Ctrl+Home | Raw Win32 edge + foreground check, `lib.rs::poll_start_chord` | READY, match not globally released |
| Select friendly Top/Jungle/Mid/Bottom/Support | F1–F5 | Raw Win32 edges, stable identity mapper | Manual control started |
| Select opposing Top/Jungle/Mid/Bottom/Support | F6–F10 | Same | Manual control started |
| Contextual move / exact-target attack | Held/swept RMB | Raw Win32 pointer + Candidate-A command | Champion selected |
| Minimap move/attack | RMB on minimap | Raw pointer + stable UI rect | Champion selected |
| Attack-move | A, then LMB | Stable `key_pressed`, raw mouse | Champion selected |
| Hold position | H | Stable `key_pressed` | Champion selected |
| Return home | B | Stable `key_pressed` | Champion selected |
| Skill 1, Skill 2, Ultimate | Q, W, R | Stable `key_pressed` | Champion selected and available |
| Confirm aimed skill | LMB | Raw mouse edge | Skill targeting active |
| Cancel targeting | RMB / Esc | Raw mouse / stable `key_pressed` | Targeting active |
| Temporary yield to vanilla AI | End | Raw Win32 edge | Champion selected; *paced match remains owned* |
| Confirmed global release | Ctrl+End | Raw Win32 edge + confirmation click | Owned match; **irreversible this match** |
| Camera grab/drag | MMB drag | Dedicated native-camera bridge + narrow WndProc shim | Match-wide, including spectator yield |
| Camera zoom | Mouse wheel | Dedicated native-camera bridge | Match-wide, including spectator yield |

`LMB` on the minimap for native camera relocation, native ordinary pause menu, and nonconflicting game menu/UI operations are **complementary vanilla behaviors**, not Harbinger replacements. Native champion-card follow remains a distinct game action; F1–F10 selection is independent of the cards, HUD visibility, player names, and native follow mappings.

## New controls to add

| New action | Proposed default | Required behavior |
| --- | --- | --- |
| Follow currently controlled champion while held | Space | Center promptly and invoke **native** follow while held; release returns to free camera |
| Toggle follow of currently controlled champion | Shift+Space | Latch/unlatch native follow; a manual MMB pan breaks the latch |
| Optionally center once without following | Unbound | Add only if a verified native center action is exposed |

A held follow always takes precedence over latched follow while held. A longer chord (Shift+Space) must take precedence over the Space-only binding so toggling cannot also fire a hold transition. On champion change, both held or latched follow retarget to the new selected athlete. On End, Ctrl+End, match exit, or no selected champion, Harbinger follow is cleared. No selected champion => Harbinger follow is a no-op.

Follow is **not currently implemented**. Prior physical tests rejected moving-target synthetic pan and synthetic F-key events. Do not resurrect them. Investigate (1) invoking a native follow widget/action through a supported SDK surface, then (2) verified native semantic follow dispatcher. See `docs/deferred-investigations.md`. Do not advertise these bindings in a release until they work.

## Native action coexistence

Confirmed v0.6.2 semantic action IDs in the current native-hook research:

| Native action | ID | Policy while Harbinger owns the match |
| --- | --- | --- |
| Highlight playback | 0x1B | Always suppress |
| Previous highlight | 0x30 | Always suppress |
| Back 10 seconds | 0x31 | Always suppress |
| Native timeline pause | 0x32 | Always suppress, keep synchronized ordinary pause menu |
| Forward 10 seconds | 0x33 | Always suppress |
| Next highlight | 0x34 | Always suppress |
| Camera zoom in/out | 0x35, 0x36 | Allow unless a specific verified, active Harbinger binding conflicts |
| Native follow own team by lane | 0x1C..0x20 | Allow if nonconflicting; suppress bound-key collisions with active Harbinger actions |
| Native follow opposing team by lane | 0x21..0x25 | Same |

The verified `src/replay_action_gate.rs` hook presently blocks **only** replay IDs. The follow IDs were recovered during v0.6.1 static research, but conditional suppression of them is **not runtime-tested**. Do not claim all game shortcut actions pass through this getter: inventory native action IDs/dispatchers and test the relevant non-replay paths before expanding suppression.

Collision policy is **semantic action + actual saved native binding + active Harbinger binding + context**, not a hard-coded Q/W/R/F-key physical blacklist. Only suppress a conflicting native action while the Harbinger action can actually run. Preserve alternate native bindings and native UI button clicks. Prioritize longer chords over contained single-key bindings. The in-menu shortcut editor must suspend gameplay hotkeys during capture. Screen-focused raw Win32 paths must continue to require TFM2 foreground and a fresh key edge after focus resumes.

Native follow on overlapping default F-keys should not move the camera incidentally just because Harbinger selects a champion. A nonconflicting user-rebound native follow key may still function independently.

The game's ordinary speed/replay features after confirmed Ctrl+End (including manual 3× diagnostic recovery) must remain available. Do not accelerate Candidate A to implement camera, binding, or presentation work.

## Config and persistence

Use one typed action registry with IDs, user labels, default keys, context, and input trigger type (press, held, toggle, gesture). Parse modifiers and mouse bindings unambiguously. Distinguish actual keyboard chord `Ctrl+End` from bare `End` with explicit precedence. Protect critical global release from accidental unbinding without an accessible restoration route; always require confirmation. Allow an explicit reset-to-defaults control.

The stable SDK's `save_*` methods are **per game save, and documented as InGame-only**; they are not automatically a globally shared shortcuts configuration. For game-wide bindings, use a separate mod-owned config file in the TFM2 user data directory with schema version, validation, defaults, and atomic writes, unless the currently installed SDK/game exposes a supported global mod-settings API. Do not write to the game's own native shortcut file.

## Native UI discovery and injection gates

1. Inspect the real v0.6.2 Shortcuts Settings **and** mod menu UI trees while the relevant panels are open. Record parent paths, template asset paths (if available), row runner kinds, scroll/list ownership, edit-button behavior, layout, focus/capture state, and reload lifecycle. A screenshot is useful, but not enough to identify the actual templates/paths.
2. Reuse the exact native tab/section and row template (via stable SDK `ui_spawn_template`) wherever available. Prefer the game's own key-capture widget/handler if it is safely reusable. Do not add custom font/color/background styling, absolute-screen overlays, or replace existing native nodes.
3. If the stable API cannot instantiate the native row or register a proper shortcut category, investigate a narrow version-checked native integration. `ui_spawn_source` using native runners is the fallback, not permission to hard-code a lookalike theme. Report any unavoidable styling or API incompatibility before shipping.
4. Idempotently create Harbinger nodes only while the appropriate settings parent exists; tolerate panel teardown/reopen, a different UI mod's layout, and a missing template gracefully. Never destroy/reparent another mod's UI.
5. Show live effective bindings, conflict warnings, Reset to Defaults, and pending key capture. Reject duplicate Harbinger bindings that would produce ambiguous behavior; warn rather than forcibly change the player's **vanilla** bindings.

Stable SDK public reference documents `ui_child_names`, `ui_runner_name`, `ui_state_json`, `ui_spawn_template`, `ui_spawn_source`, `ui_register_path_events`, and `ui_register_click`: https://github.com/teamsamoyed/TeamfightManager2Mod/blob/main/docs/stable-api-reference.md . Their availability does **not** prove a specific vanilla menu template is public or that Harbinger can register directly into the game's *native shortcut action registry*.

## Planned implementation order

1. This source/control audit; capture actual settings/mod-menu UI structure and native shortcut table.
2. Implement and test a central binding registry, global persisted configuration, keyboard/mouse action dispatch, input-editor focus gating, and default compatibility.
3. Extend semantic native conflict suppression **only** for proven action dispatchers and current bindings; preserve the existing replay protection and Ctrl+End lifecycle.
4. Integrate native shortcut rows/menu entry using shared UI templates; test with an independent UI mod and after closing/reopening settings.
5. Implement native follow once a verified follow dispatcher is available, not with custom periodic pan; ship Space/Shift+Space only then.
6. Windows v0.6.2 physical smoke tests with: defaults; changed native bindings; changed Harbinger bindings; F1–F10 both sides/red-side manager; duplicate names; hidden HUD; text input; focus loss/regain; held inputs; End vs Ctrl+End; pause; 3× only in ordinary replay; settings reentry; next match; installed UI mods; unsupported executable fail-closed.

## Release caveat

No UI rows, remapping, conditional non-replay suppression, persistent keybinds, or native follow behavior are implemented by this design document. The unmodified v0.1.5 release behavior remains authoritative until a functional PR passes build and actual game tests.
