# Contextual RMB validation

Status: contextual picking and cursor projection are physically validated. Current follow-up splits attack intent into explicit chase and exact attack execution so an out-of-range attack never falls into TFM2's higher-level combat/return behavior.

## Goal

Replace the Stage 4/5 assumption that every RMB battlefield click is a `MoveTo` with MOBA-style contextual behavior:

```text
RMB point published by client
        |
        v
paced Candidate-A StablePlayerAi callback
        |
        +--> visible targetable hostile under point --> persistent tracked target
        |                                              |
        |                                              +--> exact Attack valid -> Attack(Target)
        |                                              `--> attack ready but invalid -> MoveTo(target current position)
        |
        `--> otherwise -------------------------------> persistent MoveTo(x, y)
```

Entity resolution happens inside the Candidate-A callback through `ctx.sim()`. The render thread never guesses or persists an entity id.

## Scope

This stage intentionally supports only contextual RMB:

- empty ground -> move;
- friendly unit -> move to the clicked ground point;
- visible hostile champion/minion/tower/other targetable entity -> retain exact entity id;
- dead, untargetable, friendly, or non-visible entities cannot remain RMB attack targets;
- a chosen attack target remains identified by entity id as it moves;
- a later RMB always replaces the previous manual move/attack command;
- loss of legal vision breaks target tracking and does not auto-reacquire the unit later;
- target loss does not surrender the selected athlete back to vanilla AI.

Q/W/R targeting is deliberately not part of this stage.

## StablePlayerAi ownership rule

The official stable API states that `StablePlayerAi::think` returning `None` keeps the built-in input, while returning `Some(InputV1)` replaces it.

Therefore a manually selected athlete cannot use `None` as an idle state. When no user command is active, the control layer emits a neutral `MoveTo` to the athlete's current position. The last known self-position is retained as a fallback if one callback temporarily lacks a simulation view.

This means:

- low-health retreat logic must not seize a manually selected athlete;
- shop/recall desire must not seize a manually selected athlete;
- team macro calls such as Morgard may continue influencing all vanilla-controlled actors, but the selected athlete still receives a concrete manual input every tick;
- losing an attack target because it dies, becomes untargetable, changes team, or leaves vision transitions to neutral hold rather than built-in AI.

## Attack pursuit rule

Physical testing showed that repeatedly emitting `Attack(Target)` while the target was out of range could still result in recall/retreat-style behavior instead of literal pursuit. The stable API exposes `ctx.is_valid_input(&input)`, which checks whether the exact input is currently legal, including range/cooldown conditions, and player cooldown reads expose remaining basic-attack cooldown.

The control layer now treats RMB-on-hostile as a persistent **target intent**, not as a promise to emit `Attack` every tick:

- target not legally visible -> drop target and hold;
- exact attack currently valid -> emit `Attack(Target)`;
- exact attack invalid while basic attack is ready -> emit `MoveTo(target.pos())` and keep the target id;
- exact attack invalid while the basic attack is on cooldown -> hold current position until the attack is ready, then re-evaluate.

This avoids sending rejected attack inputs while also avoiding the undesirable behavior of walking ranged champions directly into their targets during every attack cooldown.

## Picker geometry

Stage 6A uses the entity's stable simulation `radius()` as the initial click shape. There is no extra fixed world-space tolerance yet. This is deliberate: a fixed simulation-space padding would feel different at different camera zooms.

After exact-target behavior is proven, mouse forgiveness should be specified in screen pixels and converted to simulation units from the live camera scale before publication/resolution.

## First physical result

Observed in a watched match:

1. RMB empty ground -> **PASS**.
2. RMB directly on an enemy champion -> **PASS WITH CAVEATS**.
3. RMB enemy minion -> **PASS**.
4. RMB enemy tower -> **PASS WITH CAVEATS**.
5. RMB friendly -> **PASS**.
6. Click an enemy and let it move -> **PASS WITH CAVEATS**; target identity followed the moving entity.
7. While attacking, RMB empty ground -> **PASS**.

These results are strong evidence that entity picking, relation filtering, persistent entity-id targeting, and command replacement are working.

## Second physical result

Follow-up testing reported:

- entering champion/tower threat without a new order -> **PASS**; the selected athlete could remain still under threat;
- RMB a moving enemy -> **PARTIAL PASS**; pursuit worked, but control sometimes redirected to another target;
- let the target die -> **FAIL**; vanilla AI immediately resumed control;
- RMB ground -> **PASS**;
- pan/zoom and Match Info marker alignment -> **PASS**; the yellow marker remained under the reticle.

The official `None` semantics explained the immediate handoff after target loss, so neutral hold was introduced.

## Third physical result

After neutral hold was introduced:

- low-health / combat pursuit still showed **FAIL** when a visible, out-of-range enemy was clicked: the selected athlete could immediately begin returning to base instead of moving toward the target;
- exact pursuit of a visible moving target still **FAILED intermittently** for the same reason;
- target disappearing into team fog/bush -> **PASS**: the selected athlete stopped immediately, which is the desired target-loss behavior;
- Morgard/team-call override remained **INDETERMINATE** and is intentionally deferred until generic target pursuit is reliable;
- cursor/world-marker alignment remained good.

This result motivated the attack-pursuit split above. The working hypothesis is no longer that entity selection is wrong; the problematic case is specifically an out-of-range/rejected `Attack(Target)` being allowed to interact with pre-existing higher-level behavior.

## Target-loss diagnostics

The control layer counts attack target drops by cause:

- vision loss;
- target death;
- other invalidation (missing entity, untargetable, or team mismatch).

Vision loss is intentional behavior. If an enemy enters fog/bush or otherwise ceases to be legally visible, the exact target id is discarded and will not be silently reacquired later. The player must issue a new target command.

## Cursor projection regression

The first physical test reported that the yellow world marker was offset from the UI reticle again.

Review found that `cursor_world()` had drifted away from the camera formula already physically validated in `docs/camera-research.md`. It normalized the logical UI cursor delta through the 1920x1080 UI map before applying the Game-map projection, effectively using the wrong denominator.

The follow-up restored the validated mapping:

```text
dx = mouse_ui_x - center_log_center_x
dy = mouse_ui_y - center_log_center_y

world_x = camera_center_x + dx * extent_x / Game_map_width
world_y = camera_center_y + dy * extent_y / Game_map_height
```

with `Game_map_width/height` coming from the stable `Game` draw map (observed 2048x2048). Cursor points outside `ingame.center_log` are rejected. The second physical test passed this correction across pan/zoom and Match Info layout changes.

## Current retest

After `cargo check`, `cargo test`, and reinstalling the development build:

1. At low health, RMB a visible enemy that is clearly outside attack range. The selected athlete should physically pursue that target instead of recalling/retreating.
2. Let the target move while remaining visible. Pursuit should remain attached to the same target id.
3. Once the target enters legal basic-attack range, the athlete should attack that exact target.
4. Let the target enter fog/bush. Tracking should break immediately and the athlete should hold; it must not auto-reacquire if the target reappears.
5. Let a retained target die while visible. The athlete should hold after death rather than selecting another target or recalling.

Morgard/team-call behavior can be revisited after these pass.

## Character-specific follow-up: Gunfighter

Gunfighter can move while attacking. Current generic control treats movement and attack as mutually exclusive per-tick inputs, so issuing movement during an attack causes Gunfighter to finish one attack animation and then stop attacking until another attack command is issued.

Do not special-case this until generic RMB pursuit is stable. Gunfighter will need a later command-composition/state solution that preserves its move-while-attacking identity rather than forcing the generic one-command-at-a-time model onto it.
