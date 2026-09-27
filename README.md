# Harbinger Direct Control

Real-time direct champion control for **Teamfight Manager 2**.

The mod keeps the watched match simulation running in real time, lets you take control of any of the ten visible champions, and feeds commands back through Teamfight Manager 2's own player-input system. Pick/ban, champion logic, pathing, combat resolution, fog, shopping, and the underlying simulation remain TFM2 systems; Direct Control adds a human command layer on top.

Current release target: **Teamfight Manager 2 v0.6.1 on Windows/Steam**.

## Control scheme at a glance

| Input | Behavior |
| --- | --- |
| **Ctrl+Home** | Start Direct Control once the match has synchronized and the ready prompt appears |
| **F1-F10** | Select the champion shown on that visible match card |
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

F1-F10 correspond to the **ten visible player cards**, not raw simulation player IDs. Direct Control resolves the card to stable athlete identity, so selection remains team-neutral: either side can be controlled.

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

The release camera controls are the physically validated **MMB drag + mouse-wheel zoom** path. Screen-edge scrolling and Space follow were investigated and deliberately deferred rather than shipping brittle implementations.

While a champion is controlled, Direct Control automatically switches native spectator fog to that champion's simulation team. Releasing the champion with End stops enforcement and leaves the current native spectator view in place.

A known first-release spectator loophole remains: TFM2's native follow UI can still reveal an opposing champion through fog. Direct Control is not an anti-cheat layer.

## Current compatibility notes

Physically validated on v0.6.1 include:

- real-time watched-match pacing;
- startup synchronization and Ctrl+Home handoff;
- F1-F10 selection across both teams;
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

## Automatic support diagnostics (v0.1.2)

Harbinger automatically captures startup synchronization, Ctrl+Home activation, F1-F10
selection outcomes, and a one-time player-card mapping health check while the user
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
Windows v0.6.1 have confirmed automatic logging, synchronization, successful
card mapping and champion selection. See `docs/support-diagnostics.md` for
the maintainer's interpretation guide and the final release checks.

## Technical overview

The core input path is:

```text
Windows / stable SDK input
        |
        v
visible F-key card -> stable athlete identity
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

Target platform: **Windows + Steam**. Current runtime validation applies to the tested v0.6.1 executable; an updated game build requires independent native-hook verification and physical testing.

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

### Daily workflow (one source of truth)

**Edit only this GitHub repository.** The compiled `target/` output, installed
game `mods/` directory, and staged `dist/` directory are generated copies,
not three places to manage source files.

After switching to the desired feature branch in GitHub Desktop:

```powershell
.\scripts\build-mod.ps1 -Target Dev
```

This builds from your checked-out branch and installs the test copy into the
game. Disable the Workshop installation while testing to avoid loading two
copies of the same native module.

### Workshop release

After the approved PR is merged, switch to `main` in GitHub Desktop,
**fetch and pull**, then run:

```powershell
.\scripts\build-mod.ps1 -Target Workshop
```

This **verifies main and the existing Workshop identity**, runs formatting and
tests, makes a clean default-feature release DLL, and stages the current DLL
and version metadata directly into the original publishing folder, preserving
its item ID and preview image.

Always select this exact folder in `TFM2ModUploader.exe`:

```text
<repository>\dist\workshop\tfm2_direct_control
```

Click **Refresh**, confirm the *existing* Workshop item (not `New item`),
then **Build Only (No Upload)** to inspect the package and
**Update Workshop Item** to publish. A precompiled native DLL shows
`Native code: None` in the uploader; that is expected. Preserve the
hand-edited Workshop listing description.

The exact developer, publisher, backup, duplicate-install, and recovery
procedures live in **[docs/publishing-workflow.md](docs/publishing-workflow.md)**.
Do not manually copy DLLs from the Steam installation into the publishing
folder. Keep `mod.workshop_id` in the original `dist/workshop` package
(it is intentionally Git-ignored); the packaging script makes a one-time
emergency backup outside the repository.

## Project documentation

- `CONTRIBUTING.md` — contributor setup, validation, and PR guidance
- `AGENTS.md` — current agent/contributor implementation guardrails
- `docs/publishing-workflow.md` — authoritative, single-source developer and Workshop workflow
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

## Reference

Official SDK/modding documentation: Team Samoyed's `TeamfightManager2Mod` repository, especially `docs/stable-native-mods.md`, `docs/stable-api-reference.md`, and `docs/workshop-upload.md`.
