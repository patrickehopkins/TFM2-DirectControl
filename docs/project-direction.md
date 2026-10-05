# Project direction after Control v0.9

Status: **active strategic direction, October 4, 2026.**

## Decision

Harbinger will **not** compete with Control feature-for-feature as the most polished player-facing direct-control mod.

Control has already productized several ideas Harbinger had planned—configurable controls, camera follow/lock, edge pan, an elegant selected-champion HUD, cooldown/KD presentation, current-gold display, and native next-purchase introspection. That is a successful downstream iteration from Harbinger's original work.

Harbinger's center of gravity should now move toward:

1. a small, robust, transparent reference direct-control implementation;
2. reusable low-level control/simulation infrastructure for other mods;
3. versioned TFM2 reverse-engineering/compatibility knowledge;
4. diagnostics and research tools that make difficult modding work cheaper for everyone;
5. genuinely new systems rather than player-facing parity.

The desired long-term identity is:

> **Harbinger should be the mod other ambitious TFM2 mods are built from.**

## Division of labor

### Control

Control currently appears to be the stronger destination for players who want:

- polished in-match direct-control presentation;
- configurable hotkeys through a custom overlay;
- camera peek/lock and edge pan;
- selected-champion/KD/cooldown HUD;
- current gold and native next-purchase UI;
- large champion-specific learned behavior/profile support;
- broad player-facing polish.

Harbinger does not need to recreate these solely for parity.

### Harbinger Direct Control

The shipping mod remains useful as a **reference implementation**. Priorities are:

- correct startup synchronization and Ctrl+Home ownership;
- reliable F1-F10 identity;
- generic validator-driven Q/W/R behavior;
- contextual movement/attack;
- replay safety;
- fog/camera basics;
- diagnostics;
- compatibility with new TFM2 versions.

Player-facing additions should be justified because they expose or validate reusable infrastructure, not merely because another mod has them.

### Harbinger Core / research layer

Incrementally separate reusable responsibilities behind narrow interfaces. The exact crate/file structure is not yet committed, but the conceptual components are:

- match/session ownership;
- simulation synchronization;
- manual InputV1 injection;
- stable champion/athlete identity;
- camera bridge;
- native-action discovery/suppression;
- input focus/conflict handling;
- UI discovery/native template helpers;
- compatibility/build fingerprinting;
- diagnostics.

Do not perform a giant refactor solely to achieve this shape. Extract components when active work gives a concrete reason.

## Documentation as an API

Harbinger already contains more than a normal mod README: executable probes, camera research, replay-action maps, UI paths, validation logs, compatibility scripts, and version-specific native findings.

Treat this as a deliberate **TFM2 modder research kit**.

For each supported TFM2 version, prefer documenting:

- executable/build fingerprint;
- SDK ABI;
- verified native hooks/surfaces;
- semantic native action IDs;
- useful UI paths/templates;
- known hazards;
- validation state;
- relocation/probe procedures.

Where practical, preserve the tool that produced a finding rather than only the finding itself.

## Reuse policy

The project was created with iteration/reuse in mind, but the repository currently does not make that intent sufficiently explicit.

A future documentation/legal pass should choose and add a permissive license. MIT or MIT/Apache-2.0 are reasonable candidates, but **no license is selected by this document**.

After licensing is decided, add a concise "Building on Harbinger" section explaining that control, synchronization, camera, diagnostics, and compatibility work are intended to be reused by other TFM2 mods.

## Immediate priorities

### 1. Ctrl+Home / lifecycle hardening

Control is useful as a differential test oracle because its startup/control path appears robust and Firkin was one of the early Harbinger users affected by spectator-lock/activation failures.

Do not copy Control's implementation. Reproduce the same conditions in both mods and compare lifecycle/log evidence.

Primary cases:

- immediate Ctrl+Home versus delayed activation;
- fullscreen/windowed/half-screen;
- HUD visible/hidden;
- focus loss/regain;
- red-side manager;
- End/reselect lifecycle;
- pause/menu during synchronization.

### 2. Native shortcut/keybind infrastructure

Continue the existing `feat/native-shortcut-menu` work.

Harbinger's differentiator is **native/coexisting infrastructure**, not a prettier custom overlay:

- central typed action registry;
- persisted Harbinger bindings;
- semantic suppression of genuinely conflicting native actions;
- no rewriting of player's stored vanilla shortcuts;
- native/shared templates where available;
- input-capture gating;
- reusable conflict-handling primitives.

Control's centralized keybinding model validates this architecture, but its Ctrl+K overlay is not the native-menu solution Harbinger is pursuing.

### 3. Multiplayer reconnaissance

Begin research, **not implementation**.

The first question is not "how do we build rollback?" It is:

> What exactly does TFM2 synchronize between multiplayer peers during a watched match?

See `docs/multiplayer-research.md`.

### 4. New gameplay/control surfaces

Keep these as genuine open research areas:

- AI-responsive pings / teammate calls;
- manual shopping / native shopping override;
- multiplayer external-input synchronization.

**Pings have narrowed to an AI-weighting problem.** On October 5, Firkin reported that an experimental Control path could already emit pings and hand a hard `fight` command to AI teammates, but the result was too cooperative: the command could override sensible native caution and produce suicidal commits. Harbinger's useful research target is therefore the native decision layer underneath "should I join/continue/rotate to this fight?" and a way to bias that evaluation without replacing it. A prior Flame Simulator probe also confirmed that management-side `Athlete.stat` values are readable in-match, making player-stat-to-AI-state tracing a concrete lead. See `docs/ai-ping-investigation.md`.

Manual shopping remains useful as an infrastructure problem even though Control already solved the player-facing current-gold/next-item HUD.

## Things explicitly not worth chasing right now

- copying Control's custom UI styling;
- reproducing its K/D/cooldown/selected-character HUD;
- reproducing its current-gold/next-item HUD for parity;
- building a competing per-champion empirical profile database;
- edge scrolling merely because Control has it;
- cheat/debug features unrelated to Harbinger infrastructure;
- champion-specific hard-codes where the runtime validator can remain authoritative.

## Success criterion

A future mod author should be able to spend their time on the novel behavior they care about rather than rediscovering:

- how to take ownership of a watched match;
- how to identify the correct champion;
- how to inject legal inputs;
- how to keep presentation and simulation synchronized;
- how to suppress dangerous replay behavior;
- how to survive TFM2 updates;
- how to diagnose a broken integration.

If Harbinger makes that possible, a downstream mod being more elegant or feature-rich than Harbinger is a success, not a failure.
