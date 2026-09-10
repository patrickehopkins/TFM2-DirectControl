# Candidate A pacing validation log

This file records physical validation of the staged pacing work for Teamfight Manager 2 v0.5.8.

## Stage 1 — read-only StablePlayerAi observer

Status: **PASS — physically validated 2026-09-10**.

StablePlayerAi callbacks on Candidate A's worker were proven to expose the watched simulation tick. Candidate A raced hundreds of simulated seconds ahead of presentation when unpaced, all ten player ids appeared on the same worker, and callbacks stopped when Candidate A completed.

## Stage 2A — direct `match_view + 0x250` played-tick hypothesis

Status: **FAIL — physically rejected 2026-09-10**.

The value remained exactly `1` at visible 00:02, 00:05, 00:20, and 00:30. It is not the advancing presentation tick.

## Stage 2B — adjacent playback accumulator decode

Status: **FAIL — physically rejected 2026-09-10**.

Common integer/floating-point/Duration interpretations of `match_view + 0x258/+0x260` failed closed rather than matching presentation.

## Stage 2C — ranked match-view clock scan

Status: **FAIL — physically rejected 2026-09-10**.

A read-only scan of `match_view + 0x000 .. +0x95C` found no scalar field whose change tracked the visible clock. Further playback-clock archaeology is deferred because it is not required for real-time pacing.

## Stage 3A — bounded 60 Hz Candidate-A pacing

Status: **PASS — physically validated 2026-09-10**.

Candidate A was paced from its StablePlayerAi callback at 60 ticks per monotonic wall-clock second with ~35 ms allowed lead, while `base_input` remained vanilla AI. Representative results:

```text
visible 00:15 | origin 481 | tick 1416 | elapsed 15547 ms
visible 00:20 | origin 481 | tick 1725 | elapsed 20687 ms
visible 00:25 | origin 481 | tick 2031 | elapsed 25797 ms
```

The temporary 30-second release then let Candidate A race ahead and complete normally. This proved the core pacing technique.

## Stage 3B — continuous 60 Hz full-match pacing

Status: **PASS — physically validated 2026-09-10**.

The artificial 30-second release was removed. Candidate A remained `active 1 / done 0` throughout a ten-minute watched match with no abnormal behavior and `safety fail-open no` throughout.

Representative long-run result at visible 10:00:

```text
origin tick 10 | tick 36039 | elapsed 600438 ms | safety fail-open no
```

Observed simulation delta was `36029` ticks. Exact 60 Hz over 600.438 seconds predicts ~`36026` ticks, so the simulator was only about three ticks (~50 ms) ahead after ten minutes. The fixed worker thread id within the match is expected; different matches use different worker ids.

The user tested the game's normal playback-speed controls during paced simulation. The UI retained its chosen 3x/0.5x/etc. state, but actual presentation remained effectively at 1x while Candidate A was live. This is desirable for the first direct-control version. True variable-speed live control is explicitly deferred.

## Stage 3C — Ctrl+End irreversible finish release

Status: **PASS — physically validated 2026-09-10**.

Ctrl+End was added as a deliberate one-way release. It sets `manual finish YES`, permanently disables pacing/manual control for that match, and lets Candidate A finish at native full speed. Starting a new match resets the latch.

Physical test:

- normal continuous pacing through roughly visible 00:40;
- Ctrl+End pressed once;
- overlay changed to `manual finish YES` while `safety fail-open no` remained unchanged;
- Candidate A immediately raced from the low-thousands tick range toward the end of the simulation;
- `active 0 / done 1` appeared around visible 00:47-00:48, roughly 6-7 seconds after release, consistent with normal unpaced simulation duration;
- visible replay continued normally.

This confirms the release flag is removing real simulation backpressure rather than merely changing UI state.

## Stage 4A — first real manual MoveTo

Status: **implemented; awaiting physical validation**.

The first gameplay-mutation test deliberately stays narrower than the full control scheme:

- F6-F10 select one of the user's five visible player slots, mapped to simulation player ids 5-9;
- RMB inside the verified match viewport publishes the existing camera-derived world coordinate;
- only the selected player's callback on the confirmed Candidate-A worker returns `InputV1::move_to(x, y)`;
- after the first RMB target, the same MoveTo is returned each tick until another target/player is selected;
- selection alone does not suppress vanilla AI yet, so this stage tests command delivery rather than complete manual-idle behavior;
- all other players and all non-Candidate-A simulations remain vanilla;
- Ctrl+End and safety fail-open both permanently disable manual input for the remainder of the match.

The overlay reports selected slot/player id, published target, RMB command count, manual-input return count, and last simulation tick that received the override.

Pass criteria:

1. F6-F10 selection appears correctly in the overlay.
2. One RMB in the viewport increments `RMB cmds` and records a plausible target coordinate.
3. `manual returns` begins increasing on Candidate A while pacing remains healthy.
4. The selected champion visibly changes course toward the commanded position.
5. Repeated RMB commands visibly retarget the same champion.
6. Other champions continue vanilla behavior.
7. Ctrl+End permanently stops further manual command application and lets Candidate A finish normally.

Hostile-unit click interpretation is intentionally **not** part of Stage 4A. The parallel `feat/cursor-entity-picking` branch will later distinguish ground MoveTo from hostile Attack once this basic command path is proven.

## Parallel branch note

`feat/cursor-entity-picking` was created from the Stage-1 pacing tip. Its substantive implementation remains isolated in `src/entity_picker.rs` and `docs/entity-picking.md`. It also touches `src/lib.rs`, which is now expected to require a manual merge because Stage 4A added selection/RMB wiring there. The concepts are complementary rather than conflicting: Stage 4A proves ground-command delivery; the entity-picking branch classifies what was clicked.
