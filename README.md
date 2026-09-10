# TFM2 Direct Control

Experimental direct champion control for **Teamfight Manager 2**.

The goal is to expose a small, reusable set of low-level direct-control primitives for the game's ten live actors: real-time simulation pacing, actor selection, mouse/world targeting, and native player inputs. This core deliberately does **not** decide which team a human is allowed to control. Ownership, permissions, game-mode rules, and richer policy belong in higher-level mods that may build on top of this project.

## Status

Bootstrap loading is verified on Teamfight Manager 2 v0.5.8. The watched-match simulation has been identified and continuous ~60 Hz wall-clock pacing has been physically validated for at least ten visible minutes without a safety fail-open. Native `InputV1::move_to` injection into the paced watched simulation is physically verified: selected actors respond in real time and other AI actors react to the changed behavior.

Pause/resume is also physically verified. Opening the game's pause UI freezes Candidate A; closing it resumes from a re-anchored pacing origin, so paused wall time does not become hidden catch-up simulation. Persistent MoveTo commands survive pause/resume and remain responsive afterward.

Visible F1-F10 card order is **not** Candidate A's internal `player_id` order. Selection is therefore resolved through stable athlete identity rather than arithmetic player-id assumptions. The control layer remains deliberately team-neutral.

The direct-control foundation is considered ready for **command expansion**. Higher-priority work can now proceed to attack/cast/return-home primitives without reopening the already-proven pacing architecture.

Two known polish issues are intentionally deferred: startup still requires some pre-simulation progress before the battlefield becomes independent, and the yellow world marker has a constant screen-origin offset from the physical mouse reticle. See `docs/known-issues.md`.

## Core selection contract

F1-F10 refer to the **ten visible match cards**, exactly as Teamfight Manager 2 labels them. They are convenience selectors, not raw simulation `player_id` values.

The current implementation resolves the selected card's displayed athlete to the stable athlete id, then matches `StableAiContext::athlete_id()` on Candidate A. This avoids depending on the game's opaque internal player ordering.

There is intentionally no blue-side/red-side or player-team restriction here. A future mod may impose one without changing the underlying control primitive.

## MVP control contract

| Input | Behavior |
| --- | --- |
| F1-F10 | Select the athlete shown on that visible match card |
| RMB on ground | Move the selected athlete's champion to the clicked world position |
| RMB on hostile unit | Attack that specific target; normal game movement/range behavior handles approach |
| Q | Arm Skill 1 |
| W | Arm Skill 2 |
| R | Arm Ultimate |
| LMB while a skill is armed | Confirm target / position / direction at the mouse |
| RMB or Esc while a skill is armed | Cancel the queued skill |
| B | Return to base |
| **Ctrl+End** | **Release manual control and let the simulation finish at full speed. This cannot be undone for the current match.** |

`Ctrl+Home` currently exists as an experimental startup/release diagnostic from the shelved startup-gating work. It is **not** part of the stable 1.0 control contract and higher-level consumers should not depend on zero-pre-simulation startup semantics yet.

Skill handling should follow each ability's native casting type:

- `Targeting` -> click a valid unit.
- `Position` -> click a world position.
- `Direction` -> cast from the controlled champion toward the clicked mouse position.
- `None` -> cast immediately when the key is pressed.

### Releasing manual control

`Ctrl+End` is intentionally a deliberate chord rather than a single easy-to-hit gameplay key. It permanently releases direct control for the current match and removes the real-time pacing limit, allowing Teamfight Manager 2's simulation to race to completion normally.

Once released, **manual control cannot be resumed in that match**. The watched playback can continue, but the authoritative simulation may already be far ahead or finished. Starting a new match resets the release state and permits direct control again.

The in-match diagnostic/control UI must keep this consequence visible wherever the release command is offered.

### Explicitly deferred

- Startup polish / true zero-pre-simulation match start.
- Cursor-origin calibration for precision clicking.
- Manual shopping. Vanilla automatic item purchasing remains enabled.
- Pings and sophisticated teammate orders.
- Multiplayer and replay guarantees.
- Polished targeting graphics or settings UI.
- Ownership/team permission policy in the core control primitive.
- User-facing live simulation-speed selection. Direct-control mode currently targets 1x real-time pacing; optional 0.5x/1.5x/2x/etc. live rates can be added later.

## Technical direction

Teamfight Manager 2's recommended stable native mod API exposes per-tick player-input replacement and the game's native move, attack, skill, ultimate, and return-home input types. Runtime probing confirmed that the watched-match simulation normally races far ahead of presentation, and that holding its confirmed worker near 60 ticks per wall-clock second keeps it live alongside the visible match.

The input path is:

```text
Windows mouse / keyboard state
        |
        v
visible F-key card -> stable athlete identity
        |
        v
window/client coordinates
        |
        v
mouse -> match-world transform
        |
        +----> entity hit testing
        |
        v
manual command state
        |
        v
TFM2 StablePlayerAi -> InputV1
        |
        v
watched-match simulation paced near 60 Hz
```

External live input crosses Teamfight Manager 2's deterministic simulation boundary. Until that architecture is proven replay-safe, this project is intentionally **single-player first**.

## Development setup

Target platform for the first version: **Windows + Steam**.

The GitHub working copy itself is the development workspace. It can be cloned anywhere; no separate sacrificial or `C:\Dev` copy is required. Switching branches in GitHub Desktop updates this same working directory automatically. All helper scripts resolve paths relative to the repository root.

Default Steam install used by the helper scripts:

```text
C:\Program Files (x86)\Steam\steamapps\common\Teamfight Manager2
```

The official stable SDK ships with the game under:

```text
<TFM2 install>\mod-sdk-stable\mod-api-stable
```

For local development, `scripts\bootstrap-sdk.ps1` copies that crate into the repository at:

```text
TFM2-DirectControl\
  sdk\
    mod-api-stable\
```

The entire `sdk\` directory is Git-ignored. It is a local dependency copied from the installed game and is not committed to the repository.

If PowerShell reports that script execution is disabled, allow scripts only for the current PowerShell process:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
```

Closing that PowerShell window restores the previous policy.

### Build

```powershell
cargo build --release
```

Expected Windows artifact:

```text
target\release\tfm2_direct_control.dll
```

### Install a development build

From the repository root:

```powershell
.\scripts\install-dev.ps1
```

On the first run, the script copies the stable SDK into `sdk\mod-api-stable`, builds the DLL, and installs these files under:

```text
<TFM2 install>\mods\tfm2_direct_control\
  mod.mod_info
  tfm2_direct_control.dll
```

Pass `-GameDir` if Teamfight Manager 2 is installed in another Steam library:

```powershell
.\scripts\install-dev.ps1 -GameDir "D:\SteamLibrary\steamapps\common\Teamfight Manager2"
```

The GitHub repository/folder may remain named `TFM2-DirectControl`; the installed runtime mod id is deliberately lowercase `tfm2_direct_control` so it matches the Rust DLL name and TFM2 mod folder.

## Milestones

1. Build and load the bootstrap DLL with no diagnostics. **Verified on v0.5.8.**
2. Read physical mouse position/buttons while a match is active. **Verified.**
3. Prove mouse-screen -> match-world scale with a debug marker. **Scale verified; constant screen-origin calibration remains.**
4. Identify and pace the watched-match simulation near real time. **Verified at continuous ~60 Hz for 10+ visible minutes.**
5. Pause/resume Candidate A with presentation without hidden catch-up. **Verified.**
6. Select arbitrary visible F1-F10 actors without team assumptions. **Verified through stable athlete identity.**
7. RMB ground -> native `Move` input for an arbitrary actor. **Verified in real time.**
8. Ctrl+End -> irreversibly release direct control and let the simulation finish. **Verified.**
9. Startup without unwanted pre-simulation. **Shelved for later polish; not blocking command work.**
10. RMB hostile -> native `Attack` input against the clicked entity.
11. Q/W/R + LMB normal-cast targeting.
12. B -> return to base.
13. Functional range/direction/position targeting indicators.

## Validation and known issues

- `docs/pacing-validation-log.md` — simulation/pacing/startup/pause physical test history.
- `docs/manual-control-validation.md` — manual MoveTo and actor-selection validation.
- `docs/core-control-contract.md` — low-policy, team-neutral architecture contract.
- `docs/known-issues.md` — intentionally deferred startup, cursor, speed, and command-state polish.

## Reference

Official modding documentation:

- Team Samoyed, `TeamfightManager2Mod`
- `docs/stable-native-mods.md`
- `docs/stable-api-reference.md`
- `docs/mod-package.md`
