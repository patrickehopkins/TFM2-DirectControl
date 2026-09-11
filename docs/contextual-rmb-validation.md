# Contextual RMB validation

Status: implementation prepared on `feat/contextual-rmb-targeting`; requires local Rust build and one-match physical validation.

## Goal

Replace the Stage 4/5 assumption that every RMB battlefield click is a `MoveTo` with MOBA-style contextual behavior:

```text
RMB point published by client
        |
        v
paced Candidate-A StablePlayerAi callback
        |
        +--> visible targetable hostile under point --> persistent Attack(Target(entity_id))
        |
        `--> otherwise -------------------------------> persistent MoveTo(x, y)
```

Entity resolution happens inside the Candidate-A callback through `ctx.sim()`. The render thread never guesses or persists an entity id.

## Scope

This stage intentionally supports only contextual RMB:

- empty ground -> move;
- friendly unit -> move to the clicked ground point;
- visible hostile champion/minion/tower/other targetable entity -> attack exact entity id;
- dead, untargetable, friendly, or non-visible entities cannot become RMB attack targets;
- a chosen attack target remains identified by entity id as it moves;
- if that target dies, becomes untargetable, changes team, or becomes non-visible, manual ownership of that attack command ends;
- a later RMB always replaces the previous manual move/attack command.

Q/W/R targeting is deliberately not part of this stage.

## Picker geometry

Stage 6A uses the entity's stable simulation `radius()` as the initial click shape. There is no extra fixed world-space tolerance yet. This is deliberate: a fixed simulation-space padding would feel different at different camera zooms.

After exact-target behavior is proven, mouse forgiveness should be specified in screen pixels and converted to simulation units from the live camera scale before publication/resolution.

## Build gate

From the repository root with the official stable SDK bootstrapped locally:

```powershell
cargo check
cargo test
```

Expected picker unit tests cover:

- team relation filtering;
- dead/untargetable filtering;
- visibility filtering;
- optional visibility bypass for future targeting policies;
- minimum-radius math;
- overlapping-entity nearest-center selection;
- deterministic equal-distance tie-breaking.

## One-match physical test

Use the existing Stage 5A flow unless the current `main` instructions change:

1. Start a watched match and enter the paced interactive state.
2. Select a visible athlete with the existing F-key mapping.
3. RMB several empty-ground points. Existing MoveTo behavior must remain unchanged.
4. RMB directly on a visible hostile champion. The controlled champion must target that exact champion rather than merely moving to the original click point.
5. Repeat on a hostile minion.
6. Repeat on a hostile tower when targetable.
7. RMB directly on a friendly champion/minion. It must not become an attack target; the click should remain ground movement.
8. While attacking a moving hostile, do not click again. The command must retain that entity id as the target moves.
9. Issue a ground RMB while attacking. The attack must be replaced by MoveTo immediately.
10. Let or cause the attack target to die. The stale entity id must not remain a manual attack command.

## Unknown to resolve physically

The stable API documents `InputV1::action(InputKindV1::Attack, Target(entity_id))`, but does not document whether an out-of-range attack input automatically chases the selected target.

Test both:

- hostile already inside basic-attack range;
- hostile clearly outside basic-attack range.

If in-range works but out-of-range does not chase, keep the exact target id and add a chase-to-target phase rather than falling back to nearest-target AI or discarding target identity.

## Current diagnostic note

The existing overlay still labels the RMB point as a MoveTo target because this branch deliberately avoids mixing a broad `lib.rs` presentation edit into the first command-path change. The physical behavior is the initial pass/fail signal. If targeting fails ambiguously, expose the already-collected resolver counters and active attack target id in the overlay before changing command behavior.
