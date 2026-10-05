# Harbinger Direct Control

Real-time direct champion control for **Teamfight Manager 2**.

The mod keeps the watched match simulation running in real time, lets you take control of any of the ten champions, and feeds commands back through Teamfight Manager 2's own player-input system. Pick/ban, champion logic, pathing, combat resolution, fog, shopping, and the underlying simulation remain TFM2 systems; Direct Control adds a human command layer on top.

Current Harbinger version: **v0.1.5**, targeting **Teamfight Manager 2 v0.6.2 on Windows/Steam**.

## Project direction

Harbinger remains a usable direct-control mod, but its long-term goal is no longer to win a feature-for-feature polish race. The October 2026 study of Firkin's **Control** mod showed a successful downstream iteration: Control has already productized several player-facing ideas Harbinger had planned, including configurable controls, camera peek/lock, a polished selected-champion/KD/cooldown HUD, and current-gold/native-next-purchase presentation.

Harbinger's differentiator will increasingly be **reusable infrastructure and research**: robust match ownership, simulation synchronization, stable identity, legal input injection, native-action/shortcut coexistence, compatibility tooling, diagnostics, and new control surfaces that other mods can build on.

See:
- `docs/control-0.9-study.md` — observed Control architecture/behavior and clean-room comparison;
- `docs/project-direction.md` — active division of labor and Harbinger's new strategic direction;
- `docs/multiplayer-research.md` — reconnaissance plan for synchronized external control;
- `docs/ai-ping-investigation.md` — current AI-responsive ping/stat-weighting research handoff.

## Control scheme at a glance

| Input | Behavior |
| --- | --- |
| **Ctrl+Home** | Start Direct Control once the match has synchronized and the ready prompt appears |
| **F1-F10** | Select champion: F1-F5 own team, F6-F10 opposing team; Top/Jungle/Mid/Bottom/Support order |
| **RMB** | Contextual move/attack; may be held and swept continuously |
| **RMB on minimap** | Contextual minimap move/attack |
| **A, then LMB** | Attack-move |
| **H** | Hold/stop |
| **B** | Return to base |
| **Q / W / R** | Use Skill 1 / Skill 2 / Ultimate |
| **LMB while a targeted skill is armed** | Confirm target / position / direction |
| **RMB or Esc while a targeted skill is armed** | Cancel skill targeting |
| **End** | Give the currently controlled champion back to AI and return to spectator control |
| **Ctrl+End** | Permanently release Direct Control for the current match; confirmation required |
| **MMB drag** | Grab-and-drag camera |
| **Mouse wheel** | Zoom camera |

Self-only/cursorless skills cast immediately on Q/W/R instead of requiring a redundant click on the controlled champion.

### Selection and targeting

F1-F5 select the manager's Top, Jungle, Mid, Bottom, and Support champions, respectively; F6-F10 select the opposing team's champions in the same order. The mapper links the manager's persistent club ID to the athletes' contract club IDs to determine which Candidate-A match side (blue or red) belongs to the player, then uses authoritative team/lane/athlete identities. The simulation must identify all ten athletes, but some athlete contract records can be missing; at least two consistent records for each side are required before the club-to-side mapping is trusted. It does **not** compare the club ID directly to the blue/red simulation-side ID, or depend on player names, card text, card visibility, or raw simulation player-ID ordering. Both teams are controllable, including with duplicate player names, a hidden HUD, or remapped native follow shortcuts. Harbinger's F-keys are currently fixed independent selection shortcuts; if the game also binds a native follow action to one of those keys, it may move the camera separately. Selecting a champion in Harbinger does not itself trigger camera follow.

Contextual RMB uses enlarged **clickable selection geometry only**; it does not change collision or pathing:

- Champions: +8 screen px
- Towers: +28 px
- Other targetable objectives/buildings: +24 px
- Minions: +5 px

When enlarged areas overlap, priority is **Champion > Building/Objective > Minion**. The lightweight rings shown in Direct Control represent those effective clickable regions.

**v0.1.2 targeting polish:** Entity-targeted Q/W/R skills use the same outer
click rings as RMB instead of only the native collision region. Physical testing
confirmed that this resolves the unresponsive skill-targeting edges. Bees use a
lane-minion-capped selection base with +9 px outer padding; stump and mushroom
jungle creeps use +20 px instead of the larger objective tier's +24 px.
Non-champion selection outlines use the same 3 px stroke thickness as champion
rings. These are pick/visual changes, not changes to native collision or attack
range. See `docs/click-targeting-geometry.md` for the full policy.

Held RMB continually republishes the current cursor through the same contextual resolver. Moving the cursor from ground onto an enemy, off an enemy, or onto another enemy updates the command without requiring repeated clicks.

### Skills

Direct Control asks TFM2's own runtime validator what input forms a skill currently accepts rather than hard-coding per-champion targeting rules.

- Entity-target skill -> click a valid unit.
- Position skill -> click a world position.
- Direction skill -> aim from the champion toward the cursor.
- Cursorless/self-only skill -> casts immediately on keypress.
- Hostile entity-target skill clicked out of range -> retain that exact target and chase until the cast becomes legal.
- Q/W/R pressed while the slot is locked or on cooldown -> no queued/delayed cast.

Gambler Skill 1 was investigated before release and does **not** require a special adapter: vanilla AI itself emits it as an entity-target Skill input, and physical retesting confirmed Direct Control can cast it by explicitly targeting an enemy champion.

## Match startup and release

TFM2 requires some watched-match simulation progress before it can construct the battlefield. Direct Control therefore cannot freeze the match at literal tick 1 without hanging the loader.

The validated startup path is:

1. allow the bounded loader runway;
2. freeze the live watched simulation at the first usable InGame boundary;
3. wait for visible presentation to catch that frozen live state;
4. show **“Direct Control is ready. Press Ctrl+Home to take control and resume the match.”**
5. start live control only after Ctrl+Home.

`End` is temporary: it releases only the currently controlled champion to AI and leaves Direct Control's live pacing intact.

`Ctrl+End` is global and irreversible for that match. After confirmation, Direct Control relinquishes live control/pacing and the normal simulation may race ahead or finish. Start a new match to regain Direct Control.

While Direct Control owns the live match, TFM2's native replay seek/highlight controls and their **rebindable keyboard actions** are suppressed. The native timeline-pause shortcut is also suppressed; use the ordinary synchronized pause menu instead. MMB drag and mouse-wheel camera zoom remain available. Temporary `End` spectator yield keeps replay seeking blocked because the live simulation is still paced. Native replay controls and their shortcut bindings return after confirmed `Ctrl+End` release.

### Presentation desync indicator

The lightweight targeting/click rings come from the **live simulation**, not delayed replay presentation. **If those circles are in different positions on the map from their actual entities, the displayed replay is out of sync.** The practical recovery in ordinary replay is to fast-forward until the circles and entities line up again. Direct Control deliberately disables replay seeking while active to prevent creating that divergence. If you notice desync anyway while controlling, stop issuing commands; if necessary, confirm `Ctrl+End` to restore ordinary replay fast-forward. That release is irreversible for the current match, and the Direct Control targeting circles are no longer available to compare afterward.

## Camera and fog

The release camera controls are the physically validated **MMB drag + mouse-wheel zoom** path. Camera follow on F-key selection was incidental behavior of the game's independently configurable native follow shortcuts; Harbinger does not currently center or follow the camera itself when selecting a champion. Dedicated camera-follow controls, configurable Harbinger shortcuts, screen-edge scrolling, and Space follow are deferred rather than shipping brittle implementations.

While a champion is controlled, Direct Control automatically switches native spectator fog to that champion's simulation team. Releasing the champion with End stops enforcement and leaves the current native spectator view in place.

A known first-release spectator loophole remains: TFM2's native follow UI can still reveal an opposing champion through fog. Direct Control is not an anti-cheat layer.

## Current compatibility notes

Physically validated on v0.6.2 include:

- real-time watched-match pacing;
- startup synchronization and Ctrl+Home handoff;
- F1-F10 selection across both teams, including duplicate names, remapped native follow shortcuts, and hidden HUD;
- ground movement, exact-target attacks, and held contextual RMB;
- minimap commands;
- attack-move;
- Hold and Return;
- targeted Position/Direction/Target skills;
- immediate self-only/cursorless skills;
- Berserker Skill 1 and Monk Skill 1 immediate casting;
- Ogre automatic/passive behavior remaining non-activatable;
- Gambler Skill 1 entity-target casting;
- pause/resume;
- automatic controlled-team fog;
- MMB pan and wheel zoom;
- End temporary AI release;
- Ctrl+End confirmed global release;
- background-focus safety for raw keyboard shortcuts.

Known/deferred work is tracked in:
- `docs/known-issues.md`
- `docs/deferred-investigations.md`
- `docs/champion-compatibility.md`

One notable champion-specific gap remains post-release: Gunfighter's native move-while-attacking behavior does not compose correctly with generic attack-move yet.

## Automatic support diagnostics (introduced in v0.1.2; selection hardened in v0.1.4–v0.1.5)

Harbinger automatically captures startup synchronization, Ctrl+Home activation, F1-F10
selection outcomes, and a one-time authoritative roster/team-slot mapping health check while the user
plays normally. No debug switch, command line, or extra in-game controls are necessary.
Each compiled DLL embeds a revision-and-build-time support stamp so we can
distinguish development builds without changing the public version number.
The small companion log lives alongside the game's own `log.log`:

```text
%APPDATA%\\TeamSamoyed\\TeamfightManager2\\data\\harbinger-diagnostics.log
```

If a player cannot activate or select a champion after this update, ask them only
to reproduce the problem once and send that single log file. The normal game log
records whether the companion log started successfully. Normal-play tests on
Windows v0.6.1 and v0.6.2 have confirmed automatic logging, synchronization, successful
champion mapping and selection. The v0.1.4 selection regression was also tested against duplicate names, rebound native follow shortcuts, and hidden UI. See `docs/support-diagnostics.md` for
the maintainer's interpretation guide and the final release checks.

## Technical overview

The core input path is:

```text
Windows / stable SDK input
        |
        v
F-key slot -> manager team + Candidate-A team/lane -> stable athlete identity
        |
        v
mouse -> battlefield/minimap projection
        |
        +----> contextual entity hit testing
        |
        v
manual command state
        |
        v
StablePlayerAi -> InputV1
        |
        v
watched ClientMatchView simulation paced near 60 Hz
```

External live input crosses TFM2's deterministic simulation boundary. The first public release is therefore **single-player first**. Multiplayer work remains a separate future task.

## Development setup

Target platform: **Windows + Steam**. Current runtime validation applies to the tested v0.6.2 executable; an updated game build requires independent native-hook verification and physical testing.

Prerequisites: a Windows installation of Teamfight Manager 2 with its bundled stable mod SDK, a Rust toolchain (`cargo` and `rustfmt`), and PowerShell. Python is only needed for optional investigation/compatibility utilities in `tools/`.

The official stable SDK ships with the game under:

```text
<TFM2 install>\mod-sdk-stable\mod-api-stable
```

The helper scripts copy that SDK into the Git-ignored local path:

```text
TFM2-DirectControl\sdk\mod-api-stable
```

Default Steam install used by the scripts:

```text
C:\Program Files (x86)\Steam\steamapps\common\Teamfight Manager2
```

If PowerShell script execution is disabled for the current shell:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
```

### Build

From the repository root, copy the SDK from your **installed** game before running Cargo (the local `sdk/` directory is intentionally Git-ignored):

```powershell
.\scripts\bootstrap-sdk.ps1
cargo build --release
```

Pass `-GameDir` to `bootstrap-sdk.ps1` if the game is installed in another Steam library. `scripts/install-dev.ps1` performs its own SDK bootstrap and release build, so you can alternatively use that one-command development-install route below.

Expected artifact:

```text
target\release\tfm2_direct_control.dll
```

### Install a development build

From the repository root:

```powershell
.\scripts\install-dev.ps1
```

Pass `-GameDir` when TFM2 lives in another Steam library:

```powershell
.\scripts\install-dev.ps1 -GameDir "D:\SteamLibrary\steamapps\common\Teamfight Manager2"
```

The installed mod lives under:

```text
<TFM2 install>\mods\tfm2_direct_control\
```

## Release / Workshop

Use the game's `TFM2ModUploader.exe` to **update the existing Steam Workshop item**,
not to create a second listing. The release DLL is always built from the
checked-out repository source; neither the stray root
`TFM2-DirectControl.dll` nor an older DLL in `dist/workshop` is authoritative.

After the release PRs have merged, switch to `main` and pull the latest changes
in GitHub Desktop. From the **repository root** in PowerShell:

```powershell
.\scripts\bootstrap-sdk.ps1
cargo fmt --check
cargo check
cargo test
cargo build --release
```

Build without the `replay-native-trace` diagnostic feature. A successful
`cargo build --release` writes the canonical DLL to
`target\release\tfm2_direct_control.dll`.

Stage **that exact DLL** with the current repository metadata into your
existing Workshop package. These commands intentionally refuse to proceed if
the original Workshop ID is missing:

```powershell
$pkg = ".\dist\workshop\tfm2_direct_control"
if (!(Test-Path "$pkg\mod.workshop_id")) {
    throw "Original mod.workshop_id missing. Stop: do not create a duplicate Workshop item."
}
Copy-Item ".\target\release\tfm2_direct_control.dll" "$pkg\tfm2_direct_control.dll" -Force
Copy-Item ".\mod.mod_info" "$pkg\mod.mod_info" -Force
(Get-Content "$pkg\mod.mod_info" -Raw | ConvertFrom-Json).version
Get-FileHash ".\target\release\tfm2_direct_control.dll"
Get-FileHash "$pkg\tfm2_direct_control.dll"
```

The staged metadata must say **0.1.5**, and the two DLL hashes must match.
Keep the existing preview/thumbnail assets and **preserve
`mod.workshop_id` locally**. Do not commit the Workshop ID to Git.

1. In `TFM2ModUploader.exe`, select the **existing** package folder
   `dist/workshop/tfm2_direct_control`. Run **Build Only (No Upload)** and
   inspect the staged files. This is not a replacement for the Cargo build or
   the explicit copy above.
2. Perform a short final smoke test of the intended package: READY,
   `Ctrl+Home`, selection with F1-F5 on **your** team (whether blue or red),
   F6-F10 on the opponent, movement, skills, `End`, and confirmed
   `Ctrl+End`. Also preserve the previously passed duplicate-name, remapped
   native-follow, and hidden-UI regressions.
3. Choose **Update Workshop Item** on the **original** listing. Do not choose
   new publication; a missing `mod.workshop_id` can create a duplicate item.
   The short in-game metadata may overwrite the live Workshop description.
   Check the page afterward and restore the versioned BBCode from
   `docs/workshop-description.txt` using the Steam page's **Edit title &
   description** control as necessary.
4. For a Workshop-installed smoke test, move the duplicate local development
   installation out of `<TFM2 install>\mods\tfm2_direct_control` first, then
   restart the game. Do not load development and Workshop copies simultaneously.

See `docs/workshop-publishing.md` for the distinction between the short
in-game description and the full Steam page copy. Historical first-release
checklists are not the current update procedure.

## Project documentation

- `CONTRIBUTING.md` — contributor setup, validation, and PR guidance
- `AGENTS.md` — current agent/contributor implementation guardrails
- `docs/release-week-plan.md` — historical first-release checklist (not current instructions)
- `docs/core-control-contract.md` — low-level control architecture
- `docs/control-validation-log.md` — physical control tests
- `docs/pacing-validation-log.md` — pacing/startup/pause history
- `docs/camera-controls.md` — validated camera architecture and rejected experiments
- `docs/skill-targeting.md` — skill-targeting resolver design
- `docs/champion-compatibility.md` — champion-specific findings
- `docs/deferred-investigations.md` — exact stopping points for worked-but-deferred features
- `docs/known-issues.md` — current documented limitations
- `docs/replay-native-action-analysis.md` — validated native replay-shortcut suppression and retest requirements
- `docs/keybind-plan.md` — **future proposal**; not shipping shortcut behavior
- `docs/release-scope.md` — historical scope and backlog, not current priorities
- `docs/control-0.9-study.md` — Control v0.9 behavior/architecture study and Harbinger comparison
- `docs/project-direction.md` — active post-Control project direction
- `docs/multiplayer-research.md` — multiplayer reconnaissance plan
- `docs/ai-ping-investigation.md` — AI stat access and safe ping-response weighting investigation

## Reference

Official SDK/modding documentation: Team Samoyed's `TeamfightManager2Mod` repository, especially `docs/stable-native-mods.md`, `docs/stable-api-reference.md`, and `docs/workshop-upload.md`.
