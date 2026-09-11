# Q/W/R + LMB skill targeting

Status: generic skill execution physically validated across multiple champion/skill families. Dynamic range/geometry indicators remain unfinished polish/technical work.

## Controls

- `Q` arms `InputKindV1::Skill`.
- `W` arms `InputKindV1::Skill2`.
- `R` arms `InputKindV1::Ult`.
- `B` issues an explicit Return Home order for the selected athlete. It replaces movement/attack and cancels armed skill targeting; RMB or a later successful skill cast interrupts the return order.
- `LMB` confirms the armed skill at the current battlefield cursor.
- `RMB` while a skill is armed cancels targeting and is consumed; it does not also issue a move/attack order.
- `Escape` cancels targeting.
- Arming a skill does not erase the persistent contextual-RMB move/attack order. A successful skill cast preempts that order for one simulation tick, then the prior explicit order may resume.
- Selecting a different athlete clears any armed skill.

During development, conflicting native TFM2 shortcuts may be reassigned in the game's Shortcuts menu. The finished mod should expose its own Direct Control bindings in that menu if/when a suitable integration path is implemented. The stable mod API does not currently expose a simple shortcut-row registration hook, so native settings-menu integration is deferred rather than reverse-engineered as part of skill targeting.

## Generic target-shape resolution

The render thread publishes only the armed slot and simulation-space cursor/confirmation point. The paced `StablePlayerAi` callback owns resolution.

The stable API exposes four `InputTargetKindV1` forms:

- `Target` — exact entity id;
- `Pos` — world position;
- `Dir` — direction vector from caster;
- `None` — no explicit target.

Rather than maintaining per-champion targeting tables, the simulation callback probes candidate forms through `StableAiContext::is_valid_input`. Preview inference is advisory only; each LMB confirmation re-tests legal target forms so a stale/misclassified preview cannot veto a valid cast.

A failed LMB is consumed once and leaves the skill armed. It does **not** become a delayed automatic cast if a cooldown becomes ready later; the user must click again.

## Live range requirement

`StableAction` / `StableEffectSpec` exposes generic `range` and `growth_range` metadata, but the stable runtime AI/client contexts do not currently expose the selected vanilla champion's action object directly.

**Do not hard-code per-champion or base-patch skill ranges.** Teamfight Manager 2 changes champion balance across simulated seasons and patches, including range. Any targeting indicator must therefore reflect the current effective match-state value or current input legality. A static known range is architecturally incorrect even if it matches a fresh game.

For a ready `Position` skill, the callback currently attempts to infer the effective cast radius by probing legal `Pos` inputs through `is_valid_input`, searching from the champion toward map center. If the validator remains legal all the way to the map boundary, that is treated as **range unavailable**, not as an infinite/map-sized range.

When a trustworthy finite range is available:

1. the preview aim point is clamped to the legal range edge in the cursor direction;
2. LMB first tries the literal cursor position;
3. if rejected, it can retry the clamped edge position;
4. only a validator-approved input is emitted.

## In-game targeting preview

All indicators are drawn on the existing `Game` render map, not generated imagery.

The intended generic visual grammar is:

- every non-self skill: translucent yellow ray from caster toward the current aim point;
- when a trustworthy finite current range is available: translucent sky-blue circle centered on the caster, with the yellow ray clipped to that circle;
- self/no-target skill: caster-centered marker only;
- richer AOE/projectile geometry is added only when trustworthy runtime geometry is available.

The runtime stable interface does not currently expose a generic AOE-radius or projectile-width getter for vanilla skills, so those values must not be fabricated or hard-coded from one patch.

## Validation status

Generic manual skill execution is considered **passed** based on physical testing across Pyromancer, Siegebreaker, Berserker, Executioner, Monk, and Swordmaster. The tested set covered multiple skill behaviors and all responded correctly under direct control once native key conflicts and target-form resolution were accounted for.

A previous assumption that Berserker had a broken self-cast/self-only buff was incorrect; further testing indicates the observed restriction was the skill's intended target legality rather than a direct-control failure. Likewise, earlier apparent unresponsiveness on some skills should not be treated as implementation failures when the game's own target restrictions reject the attempted target.

The following remain **separate from the passed execution path**:

- live max-range radial acquisition and ray clipping for every skill;
- AOE radius/projectile-width visualization;
- exact presentation behavior for self/no-target skills;
- future chase-to-cast behavior for targeted skills outside legal range.

## Deferred geometry polish

Do not build per-champion presentation tables solely for targeting indicators. If a future stable API exposes vanilla action/effect metadata at runtime, consume these generically:

- current Direction max range from active action/effect range;
- line/projectile width from active effect hit geometry;
- Position AOE radius from active effect shape metadata;
- other non-circular shapes as their generic effect geometry permits.

Until then, truthful generic indicators are preferred over fabricated hitboxes.

## Timeline 0x pause

The previously unidentified no-speed-selected timeline state is the game's native **`S = Pause Match`** shortcut. It pauses match presentation without opening the full pause menu, leaving the normal speed buttons unselected.

`pause_probe` treats recognized playback controls with none selected as a timeline pause so Candidate A should stop with presentation. This can now be reproduced deliberately with `S` if the pacing safeguard needs focused validation.

## Other deferred behavior

- **Auto-attack chase recovery / orb-walk timing:** do not invent our own reset timing. Observe a vanilla AI-controlled champion that is simply walking toward a moving enemy and basic-attacking it, then copy the game's own cadence for when movement resumes after each auto-attack. The engine already knows the legal attack animation/cancel/recovery timing, so the preferred solution is to mirror that behavior rather than approximate it from the full attack cooldown.
- narrow idle retaliation when an otherwise-idle selected champion is attacked in legal basic-attack range;
- Gunfighter-specific move-while-attacking command composition;
- Morgard/ping override investigation if it remains observable after explicit-command behavior is otherwise stable;
- native Shortcuts-menu integration for Direct Control bindings.
