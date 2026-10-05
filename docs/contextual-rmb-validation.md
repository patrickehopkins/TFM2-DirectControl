# Contextual RMB validation

Status: **historical validation record.** Contextual picking, cursor projection, manual ownership, vision-aware target loss, and explicit pursuit were physically validated here. The timeline-pause/pacing follow-up referenced by the original note was later completed and is part of the current synchronized pause/replay-ownership architecture; use `README.md`, `docs/core-control-contract.md`, and current source for shipping behavior.

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

The full-cooldown hold is intentionally conservative. Physical testing now shows a small but visible post-attack pursuit delay with Lancer. Treat this as later attack-recovery/orb-walk polish rather than a generic targeting failure; changing it safely requires respecting attack duration/cancel timing instead of blindly moving on the first cooldown tick.

## Picker geometry

Exact-target behavior is proven, so contextual battlefield RMB now keeps the entity's stable simulation `radius()` as the base click shape and adds **screen-space forgiveness converted through the live camera scale**. This avoids the rejected fixed-simulation-padding problem where click feel changes with zoom.

Current first-release values:

- champion: +12 px;
- tower: +28 px;
- other hostile targetable non-champion/non-tower/non-minion entities: +24 px, covering objective/building-like entities without brittle name matching;
- minion: +5 px.

When padded regions overlap, selection priority is **Champion > Building/Objective > Minion** before center-distance tie-breaking. The geometry is picker-only: collision, pathing, attack range, and every other simulation rule remain native.

Minimap contextual RMB deliberately publishes zero screen-scale padding and therefore keeps its previous exact collision geometry.

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

## Fourth physical result

After explicit chase-vs-attack resolution was introduced:

- low health + distant visible enemy -> **PASS**;
- moving visible enemy -> **PASS**, with target identity retained through pursuit;
- entering attack range -> **PARTIAL PASS / FUNCTIONALLY CORRECT**: Lancer attacked the intended target, but paused for a visibly conservative recovery interval before resuming pursuit. This matches the current full-cooldown hold policy and is now classified as polish rather than targeting failure;
- enemy entering bush/fog -> **PASS**;
- target death -> **INDETERMINATE** during this pass because test activity prevented a clean observed kill;
- selected champion no longer used skills autonomously while manually controlled;
- a commanded jungle target could die while nearby hostile units continued attacking the selected champion, and the selected champion remained idle instead of opportunistically retargeting. This is strong evidence that manual ownership is functioning as intended.

The generic control path is now highly responsive in ordinary play.

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

## Timeline playback pause

The fourth physical pass exposed a separate viewer state that is not the full pause menu: all playback-speed buttons became unselected, the visible match stopped advancing, but Candidate A continued simulating. Selecting a playback speed resumed the viewer; temporarily selecting maximum speed caught presentation back up to simulation.

The game exposes a distinct `in_game_pause_time` action and the live layout contains selectable speed widgets:

- `speed_buttons.speed05x`
- `speed_buttons.speed1x`
- `speed_buttons.speed15x`
- `speed_buttons.speed2x`
- `speed_buttons.speed3x`

`pause_probe` now treats "speed widgets are recognized but none is selected" as a timeline pause. This freezes Candidate A through the existing presentation pause gate and re-anchors the wall-clock pacer when a speed is selected again. If a future game build stops exposing these widgets as selectable, the detection fails open rather than inventing a pause.

## Current retest

After `cargo check`, `cargo test`, and reinstalling the development build:

1. Confirm ordinary contextual RMB movement/pursuit still behaves as in the fourth physical pass.
2. If the timeline 0x state can be reproduced, verify the overlay reports a timeline pause and Candidate A tick progression stops while the viewer is stopped.
3. Select a normal playback speed again. Candidate A should resume from a re-anchored pace rather than having accumulated runnable catch-up time during the pause.

The yellow-marker calibration does not need another broad validation unless it regresses again.

## Deferred generic behavior: idle retaliation

A later non-1.0 behavior should allow a manually controlled champion that is otherwise idle/holding to begin basic-attacking an enemy that attacks it while already within legal attack range. This must remain narrow defensive retaliation, not a return to general AI threat assessment or autonomous target search.

Do not implement this until the explicit-command MVP is complete.

## Character-specific follow-up: Gunfighter

Gunfighter can move while attacking. Current generic control treats movement and attack as mutually exclusive per-tick inputs, so issuing movement during an attack causes Gunfighter to finish one attack animation and then stop attacking until another attack command is issued.

Do not special-case this until generic RMB pursuit is stable. Gunfighter will need a later command-composition/state solution that preserves its move-while-attacking identity rather than forcing the generic one-command-at-a-time model onto it.
