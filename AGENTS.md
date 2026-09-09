# AGENTS.md

## Project purpose

`TFM2-DirectControl` adds direct, MOBA-style control of one Teamfight Manager 2 champion while preserving the game's surrounding match simulation.

## Product priorities

Work in this order unless the user explicitly changes it:

1. Correct mouse-to-world coordinates under the live game camera.
2. RMB ground movement.
3. RMB contextual attack on the exact hostile unit clicked.
4. Q/W/R skill arming and LMB confirmation for Targeting / Position / Direction casts.
5. B return-to-base.
6. Bare functional targeting/range indicators.
7. Selection of which friendly champion is manually controlled.
8. Pings / teammate interaction.
9. Manual shop interaction.

Automatic shopping is intentionally acceptable and must not block the basic-control milestones.

## Approved MVP controls

- RMB ground: move to cursor.
- RMB hostile: attack that target.
- Q: Skill 1.
- W: Skill 2.
- R: Ultimate.
- LMB: confirm an armed targeted/position/directional skill.
- RMB or Esc while targeting: cancel the armed skill.
- B: return to base.

Do not silently replace this with WASD movement or nearest-target auto-selection.

## Technical rules

- Use Team Samoyed's **stable native Rust API** first. Do not start new work on the deprecated classic SDK.
- Target Windows + Steam first.
- Prefer the game's own `InputV1` movement/attack/skill/return behavior over recreating combat mechanics.
- Preserve vanilla AI for every player that is not explicitly under manual control.
- Preserve vanilla automatic item purchasing until manual shop work is explicitly prioritized.
- Keep external input acquisition separate from simulation command generation.
- Treat mouse/keyboard state crossing into `StablePlayerAi::think` as a determinism risk. Until proven otherwise, label the feature single-player only and do not claim replay or multiplayer compatibility.
- Never mutate saves merely to implement direct controls.
- Avoid invasive memory patching/hooking while the stable API can accomplish the job. If stable API limitations force an external/native bridge, isolate it and document exactly why it exists.
- Favor fail-safe behavior: if cursor conversion, target resolution, or an input state is invalid, retain/pass through vanilla input rather than emitting a fabricated command.

## Development discipline

Before a functional change:

1. Read the relevant current official stable API section.
2. Keep the change narrow enough to test in one match.
3. Add useful debug logging/visualization when introducing a new coordinate or input layer.
4. Run `cargo fmt` and `cargo check` locally.
5. Do not mix unrelated refactors into control milestones.

A successful first prototype is intentionally ugly. Correct command behavior matters more than UI polish.
