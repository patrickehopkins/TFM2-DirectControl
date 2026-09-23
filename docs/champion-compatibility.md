# Direct Control champion compatibility review

This is a **pre-release pass before Pings/team commands**. The preferred policy is universal-first: investigate whether a generic Direct Control rule, stable/native action property, or target-form resolver can fix a champion-specific failure without hard-coding that champion. Only add a champion-specific exception when the underlying action genuinely behaves differently and no resilient generic rule fits.

## Generic self-only skill casting

Direct Control should not require a second click for a skill whose only meaningful target is the caster. Berserker is the primary benchmark because his self attack steroid is central to his kit and the extra self-click is disruptive.

Current generic rule:
- `CastingType::None` / validator-accepted no-target actions cast immediately on Q/W/R press;
- Targeting actions that validate on self, reject every other currently visible entity target, and reject Direction/Position forms are treated as self-only and cast immediately on self;
- ordinary ally/enemy Target skills retain click confirmation whenever another legal entity target exists;
- locked-slot and cooldown safety remain ahead of all validator probing.

This is intentionally universal rather than a Berserker-specific adapter. If physical testing shows an ordinary ally-target skill can be misclassified when no other target is presently legal, narrow or defer the heuristic rather than hard-coding broad exceptions.

## Confirmed issues

### Gambler — Skill 1

Physical testing confirms Gambler's Skill 1 does not function under Direct Control at all.

Investigation order:

1. Determine its native input/target form and whether it bypasses the ordinary Q/Target/Position/Direction pathways used by the currently validated champions.
2. Inspect whether the action requires unique state, a secondary selection/confirmation, or action metadata not currently represented by the generic skill resolver.
3. Prefer extending the universal resolver if the behavior represents a broader action family that could affect other champions.
4. If Gambler's Skill 1 is genuinely unique, implement the smallest isolated champion/action-specific adapter rather than distorting the generic skill rules.

Do not treat the current total failure as acceptable for release.

### Gunfighter — attack-move / move-while-attacking

Physical testing confirms Gunfighter does not currently compose movement and attacks correctly under `A + LMB`: the character either attacks or walks rather than preserving the intended move-while-attacking behavior.

Investigation order:

1. Revisit native action metadata such as `can_use_with_move` and any native Gunfighter-specific basic-attack/movement path.
2. Determine whether the generic attack-move implementation is unnecessarily forcing mutually exclusive Attack versus Move inputs for an action that the base game can legally execute while moving.
3. Prefer a generic rule that honors native move-compatible actions for any champion that exposes the same capability.
4. If Gunfighter uses a truly unique native mechanism, add an isolated compatibility adapter for that mechanism.

Do not change ordinary champion attack-move semantics merely to make Gunfighter work; preserve the already validated generic behavior for champions whose attacks are movement-exclusive.

## Release criterion

Before the first public release, perform a targeted champion compatibility sweep around these two confirmed failures and any additional reproducible champion-specific failures found during testing. Universal fixes are preferred, but unique behavior may receive unique handling when justified by the game's native implementation.
