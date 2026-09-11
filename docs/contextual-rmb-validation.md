# Contextual RMB validation

Status: contextual picking and cursor projection are physically validated; current follow-up replaces implicit vanilla fallback with an explicit neutral hold for the selected athlete.

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

Additional observation: low health, recall/shop desire, and a Morgard team call appeared capable of taking control. The official `None` semantics provide a concrete explanation for much of this behavior: whenever our manual command vanished, returning `None` explicitly preserved the built-in input for that tick.

The new build therefore emits neutral hold instead of `None` for a selected athlete whenever no active manual action remains.

## Target-loss diagnostics

The control layer now counts attack target drops by cause:

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

1. Select an athlete, give no command, and observe low-health / shop-ready behavior. The athlete should hold rather than retreat or recall on its own.
2. RMB a visible hostile and let it move. The exact target should remain retained while legally visible.
3. Force that target into fog/bush if practical. Tracking should break at visibility loss, then the athlete should hold; it must not auto-reacquire if the target reappears.
4. Let a retained target die. The athlete should hold after death rather than selecting another target or recalling.
5. During a Morgard/team call, keep a visible valid hostile targeted. If the selected athlete still obeys a different team directive while the attack target remains retained, that is evidence of a second control path downstream of StablePlayerAi and should be investigated separately.

The yellow-marker calibration does not need another broad validation unless it regresses again.

## Remaining unknown

The stable API documents `InputV1::action(InputKindV1::Attack, Target(entity_id))` but does not explicitly document all movement/chase behavior around that command. Physical testing shows useful pursuit, but any remaining retargeting after neutral hold is installed must be separated into:

- intentional target invalidation (especially visibility loss), versus
- a genuine downstream game/team directive overriding a still-valid final per-tick manual input.
