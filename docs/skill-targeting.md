# Q/W/R + LMB skill targeting

Status: first generic skill-targeting implementation, awaiting local compile and physical validation.

## Controls

- `Q` arms `InputKindV1::Skill`.
- `W` arms `InputKindV1::Skill2`.
- `R` arms `InputKindV1::Ult`.
- `LMB` confirms the armed skill at the current battlefield cursor.
- `RMB` while a skill is armed cancels targeting and is consumed; it does not also issue a move/attack order.
- `Escape` cancels targeting.
- Arming a skill does not erase the persistent contextual-RMB move/attack order. A successful skill cast preempts that order for one simulation tick, then the prior explicit order may resume.
- Selecting a different athlete clears any armed skill.

## Generic target-shape resolution

The render thread publishes only the armed slot and simulation-space cursor/confirmation point. The paced `StablePlayerAi` callback owns resolution.

The stable API exposes four `InputTargetKindV1` forms:

- `Target` — exact entity id;
- `Pos` — world position;
- `Dir` — direction vector from caster;
- `None` — no explicit target.

Rather than maintaining per-champion targeting tables, the simulation callback probes candidate forms through `StableAiContext::is_valid_input`. Once a valid shape is found for an armed skill, it is cached for that targeting session. LMB confirmation is also validated before an input is emitted.

A failed LMB is consumed once and leaves the skill armed. It does **not** become a delayed automatic cast if a cooldown becomes ready later; the user must click again.

## Position-skill range inference

`StableAction` / `StableEffectSpec` exposes generic `range` and `growth_range` metadata, but the stable runtime AI/client contexts do not currently expose the selected vanilla champion's action object. The implementation therefore does not hard-code base-game champion ranges.

For a ready `Position` skill, the callback infers the current effective cast radius by binary-searching legal `Pos` inputs through `is_valid_input`, searching from the champion toward map center. This automatically reflects the game's current legality checks and level-scaled range without a champion-specific table.

When the cursor lies outside the inferred range:

1. the preview target marker is clamped to the edge of the legal range in the cursor direction;
2. LMB first tries the literal cursor position;
3. if rejected, it retries the clamped edge position;
4. only a validator-approved input is emitted.

## In-game targeting preview

All indicators are drawn on the existing `Game` render map, not generated imagery.

### Position

- translucent sky-blue filled circle centered on the selected champion = inferred maximum cast range;
- translucent yellow marker = actual target position after max-range clamping.

The runtime stable interface does not expose a generic AOE-radius getter for vanilla effect bodies, so the yellow marker is intentionally only a placement marker in this build. Do not interpret its radius as damage/effect radius.

### Direction

- translucent yellow straight line from champion center toward the cursor.

The data model contains action/effect range and effect-specific projectile geometry, but the runtime context does not expose those fields generically for vanilla actions. Directional line length therefore follows the cursor in this build, and width is a small screen-scaled presentation width rather than a claimed projectile hitbox.

### Target / unknown

- translucent yellow marker at the cursor.

### None/self

- translucent yellow marker centered on the selected champion.

## First validation targets

1. Existing RMB ground movement and hostile targeting still work when no skill is armed.
2. Q, W, and R each arm the expected slot and show `SKILL: armed ...` in the overlay.
3. RMB or Escape cancels an armed skill without moving the champion.
4. LMB on an in-range targeted skill casts on the clicked legal entity.
5. A Direction skill casts in the cursor direction and shows the yellow direction line.
6. A Position skill shows the blue range circle and yellow target marker.
7. Moving the cursor outside a Position skill's range clamps the yellow target marker to the blue edge; LMB casts at that clamped point if the game validates it.
8. A self/no-target skill can be confirmed with LMB and does not require an entity.
9. Invalid confirmation leaves the skill armed and increments the reject counter rather than handing control back to vanilla AI.
10. After a successful cast, the previous persistent RMB order can resume.

## Deferred geometry polish

Do not build per-champion presentation tables solely for targeting indicators. If a future stable API exposes vanilla action/effect metadata at runtime, consume these generically:

- Direction max range from action/effect range;
- line/projectile width from the effect's hit geometry;
- Position AOE radius from effect shape metadata;
- other non-circular shapes as their generic effect geometry permits.

Until then, truthful generic indicators are preferred over fabricated hitboxes.

## Deferred existing edge case: timeline 0x pause

A watched match once entered a state distinct from the full pause menu in which all normal playback-speed buttons were unselected, the visible timeline stopped, but Candidate A continued simulating. Selecting a speed resumed presentation and temporarily using high speed caught presentation back up.

The triggering shortcut/path is unknown. `pause_probe` now treats recognized playback controls with none selected as a timeline pause so Candidate A should stop too. Reproduction is not a prerequisite for skill work; validate this safeguard opportunistically if the state occurs again.

## Other deferred behavior

- attack recovery/orb-walk timing (current full basic-attack cooldown hold is conservative);
- narrow idle retaliation when an otherwise-idle selected champion is attacked in legal basic-attack range;
- Gunfighter-specific move-while-attacking command composition;
- Morgard/ping override investigation if it remains observable after explicit-command behavior is otherwise stable.
