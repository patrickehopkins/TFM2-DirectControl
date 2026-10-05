# Direct Control champion compatibility review

This records champion-specific compatibility findings from the first Workshop release through the current v0.1.5 / TFM2 v0.6.2 state. The preferred policy is universal-first: investigate whether a generic Direct Control rule, stable/native action property, or target-form resolver can fix a champion-specific failure without hard-coding that champion. Only add a champion-specific exception when the underlying action genuinely behaves differently and no resilient generic rule fits.

## Generic self-only skill casting

Direct Control should not require a second click for a skill whose only meaningful target is the caster. Berserker is the primary benchmark because his self attack steroid is central to his kit and the extra self-click is disruptive.

Current generic rule:
- `CastingType::None` / validator-accepted no-target actions cast immediately on Q/W/R press;
- Targeting actions that validate on self, reject every other currently visible entity target, and reject Direction/Position forms are treated as self-only and cast immediately on self;
- ordinary ally/enemy Target skills retain click confirmation whenever another legal entity target exists;
- locked-slot and cooldown safety remain ahead of all validator probing.

This is intentionally universal rather than a Berserker-specific adapter. **Original physical validation on v0.6.1, preserved by the current v0.6.2 release behavior:** Berserker Skill 1 and Monk Skill 1 cast immediately on keypress; Ogre's automatic/passive trigger did not become manually activatable. If later testing shows an ordinary ally-target skill can be misclassified when no other target is presently legal, narrow or defer the heuristic rather than hard-coding broad exceptions.

## Confirmed issues

### Gambler — Skill 1 — cleared

The earlier "completely nonfunctional" report was a testing misunderstanding rather than a Direct Control regression.

A temporary stable-API probe on `fix/gambler-skill1` captured the vanilla AI's own `base_input` when Gambler used Skill 1:

- input kind: Skill 1;
- target form: **Target**;
- a concrete enemy entity id is supplied;
- position and direction fields are unused.

Physical retest then confirmed Direct Control can cast Gambler Skill 1 by targeting an enemy champion. No Gambler-specific implementation change is required. The skill may read as though its follow-up attacks automatically choose enemies, but the native action still begins from an explicit entity-target input.

The probe branch is investigation-only and must not be merged into release `main`.

### Gunfighter — attack-move / move-while-attacking

Physical testing confirms Gunfighter does not currently compose movement and attacks correctly under `A + LMB`: the character either attacks or walks rather than preserving the intended move-while-attacking behavior.

Investigation order:

1. Revisit native action metadata such as `can_use_with_move` and any native Gunfighter-specific basic-attack/movement path.
2. Determine whether the generic attack-move implementation is unnecessarily forcing mutually exclusive Attack versus Move inputs for an action that the base game can legally execute while moving.
3. Prefer a generic rule that honors native move-compatible actions for any champion that exposes the same capability.
4. If Gunfighter uses a truly unique native mechanism, add an isolated compatibility adapter for that mechanism.

Do not change ordinary champion attack-move semantics merely to make Gunfighter work; preserve the already validated generic behavior for champions whose attacks are movement-exclusive.

## Release status

Gambler Skill 1 remains cleared in the current release. Gunfighter's move-while-attacking behavior remains the documented champion-specific compatibility gap. Universal fixes remain preferred when champion-specific work resumes.
