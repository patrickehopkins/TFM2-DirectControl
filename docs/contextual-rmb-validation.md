# Contextual RMB validation

Status: first physical pass strongly validates contextual picking; follow-up build tightens manual authority and restores the previously validated cursor projection.

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
- a later RMB always replaces the previous manual move/attack command.

Once an athlete is selected for manual control, vanilla `base_input` must remain suppressed for that athlete until manual control is explicitly released or another athlete is selected. Losing an attack target means no manual action that tick; it does not hand authority back to vanilla AI.

Q/W/R targeting is deliberately not part of this stage.

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

The caveat was apparent vanilla AI takeover around champions/towers: the selected character could move away, chase, or use skills outside the user's command. Review found a concrete cause in the first implementation: when the active attack ceased to produce a manual input, the pacing hook fell through to `base_input`. The follow-up revision treats selection as authoritative manual ownership and returns the control layer's `Option<InputV1>` directly, so `None` suppresses vanilla input rather than resuming it.

## Cursor projection regression

The same first physical test reported that the yellow world marker was offset from the UI reticle again.

Review found that `cursor_world()` had drifted away from the camera formula already physically validated in `docs/camera-research.md`. It normalized the logical UI cursor delta through the 1920x1080 UI map before applying the Game-map projection, effectively using the wrong denominator.

The follow-up restores the validated mapping:

```text
dx = mouse_ui_x - center_log_center_x
dy = mouse_ui_y - center_log_center_y

world_x = camera_center_x + dx * extent_x / Game_map_width
world_y = camera_center_y + dy * extent_y / Game_map_height
```

with `Game_map_width/height` coming from the stable `Game` draw map (observed 2048x2048). Cursor points outside `ingame.center_log` are rejected.

## Follow-up physical test

After `cargo check`, `cargo test`, and reinstalling the development build:

1. Select one athlete and issue ground RMB. Confirm normal movement.
2. Do not issue another command while entering champion/tower threat. The selected athlete must not autonomously cast skills, retreat, chase a different unit, or otherwise resume vanilla decisions.
3. RMB an enemy champion and let the target move. Confirm exact-target pursuit/attack remains attached to that entity.
4. Let the target die or otherwise become invalid. The selected athlete should stop receiving that attack command without vanilla AI immediately choosing a new action.
5. RMB empty ground afterward. Confirm control resumes immediately.
6. Pan and zoom while moving the cursor around the battlefield. The yellow Game-space marker should remain centered under the UI reticle.
7. Repeat the marker check with Match Info / alternate battlefield layout if available.

The overlay now shows the latest RMB simulation point, active attack target id, move/attack resolve counters, and attack-input return count to separate picking failures from command-execution failures.

## Unknown to resolve physically

The stable API documents `InputV1::action(InputKindV1::Attack, Target(entity_id))`, but does not explicitly document whether an out-of-range attack input automatically chases the selected target.

The first physical pass suggests the command behaves usefully enough to pursue/attack moving targets, but the follow-up test should still distinguish intentional target pursuit from any residual vanilla AI behavior now that manual authority is strict.
