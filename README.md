# TFM2 Direct Control

Experimental direct champion control for **Teamfight Manager 2**.

The goal is to turn one champion on the user's team into a manually controlled MOBA-style character while leaving the rest of Teamfight Manager 2's simulation, teammates, and automatic item purchasing intact.

## Status

Bootstrap loading is verified on Teamfight Manager 2 v0.5.8. The current functional milestone is live mouse diagnostics, followed by precise mouse-to-world tracking and right-click movement.

## MVP control contract

| Input | Behavior |
| --- | --- |
| RMB on ground | Move to the clicked world position |
| RMB on hostile unit | Attack that specific target; normal game movement/range behavior handles approach |
| Q | Arm Skill 1 |
| W | Arm Skill 2 |
| R | Arm Ultimate |
| LMB while a skill is armed | Confirm target / position / direction at the mouse |
| RMB or Esc while a skill is armed | Cancel the queued skill |
| B | Return to base |

Skill handling should follow each ability's native casting type:

- `Targeting` -> click a valid unit.
- `Position` -> click a world position.
- `Direction` -> cast from the controlled champion toward the clicked mouse position.
- `None` -> cast immediately when the key is pressed.

### Explicitly deferred

- Manual shopping. Vanilla automatic item purchasing remains enabled.
- Pings and sophisticated teammate orders.
- Multiplayer and replay guarantees.
- Polished targeting graphics or settings UI.

## Technical direction

Teamfight Manager 2's recommended stable native mod API already exposes per-tick player-input replacement and the game's native move, attack, skill, ultimate, and return-home input types. The remaining foundational problem is translating the Windows mouse cursor into the correct match-world position under the game's current camera.

The planned input path is:

```text
Windows mouse state
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
```

External live mouse input crosses Teamfight Manager 2's deterministic simulation boundary. Until that architecture is proven replay-safe, this project is intentionally **single-player first**.

## Development setup

Target platform for the first version: **Windows + Steam**.

The repository itself is the development workspace. It can be cloned anywhere; no separate sacrificial or `C:\Dev` copy is required. All helper scripts resolve paths relative to the repository root.

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

## First milestones

1. Build and load the bootstrap DLL with no diagnostics. **Verified on v0.5.8.**
2. Read physical mouse position/buttons while a match is active.
3. Prove a correct mouse-screen -> match-world coordinate transform with a debug marker.
4. RMB ground -> native `Move` input for one predetermined player slot.
5. RMB hostile -> native `Attack` input against the clicked entity.
6. Q/W/R + LMB normal-cast targeting.
7. B -> return to base.
8. Functional range/direction/position targeting indicators.

## Reference

Official modding documentation:

- Team Samoyed, `TeamfightManager2Mod`
- `docs/stable-native-mods.md`
- `docs/stable-api-reference.md`
- `docs/mod-package.md`
