# Candidate A pacing validation log

This file records physical validation of the staged pacing work for Teamfight Manager 2 v0.5.8.

## Stage 1 — read-only StablePlayerAi observer

Status: **PASS — physically validated 2026-09-10**.

Build intent:

- keep the known-good Candidate A/B/C entry probes;
- keep the known-good camera probe;
- register a `StablePlayerAi` on all players;
- identify callbacks that execute on Candidate A's active worker thread;
- record `ctx.tick()`, player id, callback count, and worker thread id;
- return `base_input` unchanged;
- do **not** sleep, pace, select a player, or emit manual `InputV1`.

### Physical result

Two screenshots from one normal watched match confirmed the intended relationship.

While Candidate A was active at visible `00:03`:

```text
A:data.rs:6143 ... active 1 done 0 | thread 20372
AI OBSERVER: total 1796036 | Candidate A 207919 | thread 20372 | players mask 0x3FF
Candidate A ctx.tick(): 516 -> 22869
```

After Candidate A completed at visible `00:07`:

```text
A:data.rs:6143 ... active 0 done 1 | thread 20372 | last 6890 ms
AI OBSERVER: total 4618519 | Candidate A 431369 | thread 20372 | players mask 0x3FF
Candidate A ctx.tick(): 516 -> 50711
```

The second screenshot was taken roughly half a second after the active-to-done transition; that timing does not affect the Stage-1 conclusion.

At 60 ticks/s, tick `22869` is about 381.15 simulated seconds while only three visible seconds had elapsed. Tick `50711` is about 845.18 simulated seconds while only seven visible seconds had elapsed. This confirms Candidate A is the watched-match simulation and races hundreds of simulated seconds ahead of presentation.

`players mask 0x3FF` confirms all ten player ids were observed on the Candidate-A worker. The observer's thread id exactly matched Candidate A's worker thread id. Candidate-A callback/tick advancement ceased after Candidate A completed while visible playback continued. No input mutation was enabled.

### PASS criteria

All criteria passed:

1. Game launched normally and reached the watched match.
2. Candidate A entered once and was active during the early visible match.
3. `AI OBSERVER total` increased.
4. `AI OBSERVER Candidate A` increased while Candidate A was active.
5. Observer thread id matched Candidate A's displayed worker thread id.
6. `Candidate A ctx.tick()` increased monotonically far ahead of the visible clock.
7. Candidate-A observer count/tick stopped advancing when Candidate A completed.
8. Visible playback continued normally after Candidate A completed.
9. No manual `InputV1` mutation was enabled.

## Stage 2 — exact played-tick observation

Status: **next test**.

Revalidate the statically traced match-view played tick at `match_view + 0x250`. The camera-containing object is already traced as `match_view + 0x960`, so the known-live camera capture can provide the owning match-view address without a new global hook.

This stage remains read-only. Its purpose is to compare authoritative Candidate-A `ctx.tick()` directly with the exact presentation tick instead of the coarse visible `MM:SS` string.

Expected relationship:

```text
played_tick ~= visible_seconds * 60
simulation_tick >> played_tick while Candidate A runs unpaced
```

The exact field must advance monotonically with visible playback, respond to the game's playback-speed controls in the same direction as presentation, and remain plausible at 60 ticks/s. Do not use it for pacing until physically validated.

## Stage 3 — bounded pacing

Status: **not implemented yet**.

Only after Stages 1 and 2 pass, add cooperative waiting on one designated Candidate-A callback per simulation tick. Initial rule:

```text
if simulation_tick > played_tick + allowed_lead:
    wait briefly and re-check
```

Start with a conservative lead window and a bounded wait/fail-open path so the worker can never deadlock the match if presentation state disappears.

No manual `InputV1` should be reconnected until bounded pacing is physically stable.

## Parallel branch note

`feat/cursor-entity-picking` was created from the Stage-1 pacing branch tip and therefore already contains the Stage-1 observer. Its substantive work is isolated in `src/entity_picker.rs` and `docs/entity-picking.md`; it also has a small wiring change in `src/lib.rs`. Future pacing work should remain modular. A later merge may require a small manual resolution in `src/lib.rs`, but no architectural conflict is currently expected.
