# Control v0.9 study and Harbinger comparison

Status: **research record / external-mod comparison, October 4, 2026.**  
Workshop item: **Control**, Steam Workshop ID `3813636149`, author Firkin.  
This document records what was actually observed in the Workshop package, runtime logs/configuration, and maintainer playtesting. It is not an instruction to copy Control's implementation.

## Why this study exists

Control is explicitly descended from Harbinger Direct Control as an idea and credits Harbinger as its starting point. Firkin was also one of the early Harbinger users who reported the Ctrl+Home/spectator-lock failure mode. The useful question is therefore not whether the two mods overlap—they obviously do—but what Control independently solved well enough to inform Harbinger's future architecture and diagnostics.

The project decision after this study is **not** to race Control feature-for-feature. Harbinger should remain a robust direct-control reference implementation while increasingly exposing reusable low-level machinery and reverse-engineering knowledge for other TFM2 mods. See `docs/project-direction.md`.

## Package shape

The downloaded Workshop item contained:

- one stripped native Rust DLL, `tft2_pov.dll` (roughly 8 MB in the inspected build);
- `mod.mod_info`;
- preview/thumbnail images;
- a large `fx/` asset tree containing roughly 2,700 SVGs.

No Rust source, source-map-equivalent project files, or reusable license were shipped in the Workshop package. The attractive settings/control presentation is therefore not an exposed layout/config file that Harbinger can simply reuse.

After runtime use, Control creates its own data directory:

```text
%APPDATA%\TeamSamoyed\TeamfightManager2\data\tft2_pov\
```

Observed files include:

- `keybindings.json`
- `profiles.json`
- `skill_modes.json`

The earlier static string `tmpkeybindings.json` is not the live persisted filename.

## User-facing UI and controls

Control's Ctrl+K interface is a custom in-match overlay with separate **Settings** and **Keys** tabs. It is not injected into TFM2's native Shortcuts settings screen.

Observed settings include:

- auto-attack when idle;
- attack-move on key press;
- cast-on-release and quick-cast behavior;
- attack-range display and damage numbers;
- mouse-wheel zoom and edge pan;
- camera pan speed;
- full vision and several cheat/debug toggles.

Observed key actions include:

- Q/W/R skill inputs;
- attack-move;
- champion-only targeting modifier;
- kite;
- hold;
- stop;
- return to base;
- attack-range hold;
- scoreboard;
- center camera while held;
- lock/unlock camera;
- directional camera pan;
- Ctrl+Home take control;
- Ctrl+End stop controlling;
- End champion back to AI;
- Ctrl+K hotkey menu;
- pause;
- fixed F1-F5 champion selection.

The persisted `keybindings.json` confirms a centralized action/config model rather than one-off hard-coded checks. It also documents fixed behaviors such as F1-F5 champion selection, Shift+skill preview, Alt+skill self-cast, RMB move/attack, and MMB drag.

### Presentation HUD

Maintainer playtesting confirmed that Control already has the economy and status HUD concepts Harbinger had planned:

- current gold;
- native next intended item/purchase information;
- K/D;
- cooldown tracking;
- currently selected champion information.

The implementation was described as elegant and behaving correctly in normal use. This means **current-gold / native-next-purchase UI is no longer a useful Harbinger differentiation target**. Harbinger may still need the underlying native economy introspection for manual-shopping infrastructure, but it should not prioritize reproducing Control's player-facing HUD merely to match it.

## Runtime layout ownership and Hide UI

Control does not solve hidden native UI in the same way Harbinger does.

When armed, Control switches the match into its own fullscreen/clean-screen presentation. Runtime logs explicitly record the clean layout transition. In maintainer testing, the game's Hide UI option therefore did not expose the same class of failure that historically affected Harbinger's card/name-dependent selection.

This matters when comparing selection reliability: Control's successful hidden-UI behavior is partly a consequence of taking presentation/layout ownership, whereas Harbinger's current F1-F10 mapper intentionally remains independent of card text, card visibility, duplicate names, and native follow bindings.

## Input interception and conflicts

Control installs a Win32 WndProc hook at startup. Its DLL imports/uses `SetWindowLongPtrW` and `CallWindowProcW`, and runtime logging reports `WNDPROC hook installed`.

This strongly suggests an input-interception layer in addition to ordinary key polling. It is a useful architectural reference for temporary physical-key suppression, but Harbinger should continue preferring **semantic native-action suppression** when a verified TFM2 action dispatcher is available.

Maintainer test:

- `Center camera (hold to peek)` was rebound to `Z`.
- `Z` normally has a vanilla camera function.
- Control's center-camera behavior worked normally.

This is suggestive of conflict suppression, but it is **not yet a conclusive differential test** because Control's fullscreen/layout ownership can hide or replace some vanilla camera effects. If this question is ever resumed, use an unmistakable vanilla action and compare behavior while Control is armed versus after Ctrl+End release.

## Camera behavior

Control has the two camera-follow interactions Harbinger wanted:

- **Center camera (hold to peek)** — Space by default; user-remappable.
- **Lock / unlock camera** — Y by default.

Runtime logs confirm that manual panning breaks the lock: `CAMERA unlocked by panning`.

This validates the desired Harbinger UX even though the implementation path is different:

1. held follow temporarily overrides free camera;
2. release returns to free camera;
3. toggle follow latches;
4. manual MMB pan breaks the latch;
5. switching selected champion should retarget follow.

Control also implements edge pan and wheel zoom.

Static/runtime evidence indicates that Control derives/repairs camera state partly from the minimap camera rectangle, including fallback/reconstruction when one edge cannot be read. Harbinger should **not** copy that presentation-derived camera architecture if a native semantic follow route can be found. The behavioral UX is worth copying; the pixel/minimap-tracking machinery is not a preferred Harbinger dependency.

## Simulation / presentation synchronization

Control and Harbinger solve the same fundamental problem with different pacing philosophies.

Observed Control lifecycle:

```text
match discovered
    ->
view synchronization begins / sim held
    ->
managed team established
    ->
presentation clock becomes live
    ->
Control armed
    ->
champion selected
    ->
manual input
```

Control runtime logs include:

- holding simulation at an early tick until presentation clock is available;
- detecting when the client is silent and temporarily pacing simulation at real time;
- returning to playback-following behavior when the client resumes;
- detecting playback starvation and reporting measured lead;
- an `InStep` rule that can trip when simulation and presentation diverge;
- playback-speed rule checks.

Harbinger's accepted invariant remains different:

- Candidate A is authoritative;
- authoritative simulation remains at the validated 60 Hz / 1x baseline;
- 3x remains a manual ordinary-replay diagnostic/recovery tool after release;
- do not repair presentation drift by accelerating authoritative simulation.

What is worth studying from Control is the **detection side**: measuring presentation-vs-simulation lead, recognizing starvation/drift, and automatically re-aligning presentation. That directly supports Harbinger's deferred snap-to-live watchdog.

## Champion selection observations

Control exposes fixed F1-F5 selection for the managed team rather than Harbinger's F1-F10 both-team model.

One observed runtime edge case is important:

```text
CONTROL F1 -> player 5 team 1 lane 0 champ 18446744073709551615
```

`18446744073709551615` is `usize::MAX` on 64-bit Windows and is effectively an invalid/sentinel champion ID. It appeared after returning a champion to AI and then selecting again in an earlier test session.

This does **not** establish that Control's selection is generally unreliable; the match continued. It does show that Control's selection/state architecture is not something Harbinger should copy blindly. Harbinger's current authoritative team/lane/athlete identity mapping remains the preferred direction, especially for both-team control, duplicate player names, hidden HUD, and remapped native follow keys.

## Learned champion profiles

Control's largest technical departure from Harbinger is its empirical learning/profile system.

`profiles.json` is extremely large and contains accumulated probe results for champion actions, buffs/debuffs, accepted/refused input forms, ranges, cooldowns, movement outcomes, target relations, and other observed behavior. Some actions have been probed hundreds of thousands or more than a million times.

`skill_modes.json` distills champion/skill behavior into explicit modes such as:

- `self`
- `position`
- `direction`
- `target`

with additional side/unit restrictions and some dash behavior.

This is a legitimate and powerful architecture, but Harbinger deliberately prefers a different generic principle: ask TFM2's live validator what the selected action accepts **now**, rather than maintaining a giant per-champion knowledge base. Do not chase Control's profile corpus as a roadmap requirement.

## Static comparison with Harbinger source

A static comparison was performed against Harbinger's current and release-era source.

The overlap is unmistakable at the **concept and reverse-engineering** level:

- Ctrl+Home / Ctrl+End lifecycle;
- End temporary AI return;
- Q/W/R, A, H, B, RMB, F-key control vocabulary;
- live watched-match manipulation;
- the same broad replay/presentation problem;
- many of the same TFM2 UI/action paths and native surfaces.

However, the inspected Control v0.9 DLL did **not** show positive evidence of current verbatim Harbinger source:

- 22 distinctive Harbinger diagnostic/UI strings from the release-era implementation were searched and none matched;
- Harbinger-specific identifiers such as `tfm2_direct_control`, `Harbinger`, `candidate_a`, `pacing_probe`, `slot_mapping`, `replay_action_gate`, `camera_probe`, and `entity_picker` were not found;
- embedded Rust module/source names show a substantially different project layout, including modules such as `sync.rs`, `executor.rs`, `input.rs`, `keys.rs`, `hotkeys.rs`, `camera.rs`, `savepatch.rs`, `profile.rs`, `learn.rs`, `layout.rs`, `overlay.rs`, and `style.rs`.

Do **not** convert this into a claim about percentage of copied code. A stripped optimized Rust DLL cannot support that conclusion responsibly. The defensible statement is:

- Harbinger conceptual DNA: obvious and acknowledged;
- Harbinger reverse-engineering knowledge: clearly useful to Control;
- identifiable current verbatim Harbinger source inside Control v0.9: not established by this inspection;
- Control's current architecture: substantially independently evolved.

## Clean-room / licensing boundary

The Workshop package did not ship source or a reuse license.

Harbinger may:

- observe behavior;
- inspect runtime logs/configuration;
- compare architecture;
- independently implement useful ideas;
- use Control as a differential test oracle.

Harbinger should **not** copy Control DLL code or SVG assets absent permission/license.

Likewise, if Harbinger's goal is to become a foundation for other modders, its own reuse terms should eventually be made explicit with a permissive license. The exact license choice remains a maintainer decision; this research pass does not add one.

## What Control has already done well enough that Harbinger should not chase it

Do not prioritize parity for parity's sake in these areas:

- custom polished in-match settings overlay;
- broad player-facing HUD polish;
- current gold / next-purchase HUD;
- K/D and cooldown HUD;
- selected-champion status panel;
- edge scrolling;
- champion-specific empirical profile database;
- large per-champion skill-behavior catalog;
- cheat/debug toggles.

Harbinger may still expose reusable low-level primitives behind some of these systems if they are valuable to other mods.

## What remains directly useful to investigate

If additional Control investigation is performed, keep it narrow and tied to Harbinger infrastructure:

1. **Ctrl+Home differential testing**
   - reproduce a Harbinger startup/control failure under the same conditions in Control;
   - compare both logs around match discovery, synchronization, arm, selection, and first manual input;
   - use Control as an oracle to eliminate whole classes of lifecycle causes.

2. **Presentation-drift detection**
   - understand what reliable clock/state Control uses to determine lead/starvation;
   - recreate the concept independently around Harbinger's 60 Hz authoritative model.

3. **Input conflict behavior**
   - only if needed for native shortcut hardening, perform one unmistakable armed-vs-released collision test.

There is currently no reason to continue broad reverse engineering of Control's player-facing features.

## Ctrl+Home differential test matrix

If Harbinger still has an activation/spectator-lock report, compare Harbinger and Control under the same conditions:

| Condition | Why it matters |
| --- | --- |
| normal fullscreen | baseline |
| half-screen/windowed | historical report condition |
| native HUD visible | baseline UI |
| native HUD hidden | layout dependency |
| custom vanilla follow/F-key bindings | shortcut dependency |
| duplicate player names | identity dependency |
| Ctrl+Home immediately on match appearance | readiness race |
| Ctrl+Home after several seconds | timing comparison |
| focus loss/regain before activation | raw-input edge state |
| red-side manager | side/ownership mapping |
| End then reselect/re-arm path | lifecycle cleanup |
| pause/menu during startup synchronization | presentation ownership |

The goal is not to make Harbinger mimic Control's state machine. The goal is to identify which prerequisite Harbinger is waiting on when Control has already become safely interactive.

## Bottom line

Control is now the stronger **polished player-facing direct-control experience** in several areas. That is a success case for Harbinger's original open/iterable intent, not a reason to duplicate it.

Harbinger's best response is to keep its reference implementation reliable and move its center of gravity toward reusable control/simulation infrastructure, compatibility research, diagnostics, and new systems that have not already been productized elsewhere.
