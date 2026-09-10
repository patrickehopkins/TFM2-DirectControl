# Candidate A pacing validation log

This file records physical validation of the staged pacing work for Teamfight Manager 2 v0.5.8.

## Stage 1 — read-only StablePlayerAi observer

Status: **prepared; awaiting physical test**.

Build intent:

- keep the known-good Candidate A/B/C entry probes;
- keep the known-good camera probe;
- register a `StablePlayerAi` on all players;
- identify callbacks that execute on Candidate A's active worker thread;
- record `ctx.tick()`, player id, callback count, and worker thread id;
- return `base_input` unchanged;
- do **not** sleep, pace, select a player, or emit manual `InputV1`.

### Expected overlay

During a normal watched match the diagnostic panel should show:

```text
SIM TASK PROBE: A confirmed watched-match job | Candidate-A AI observer READ ONLY
...
AI OBSERVER: total <increasing> | Candidate A <increasing> | thread <same as A> | players mask <nonzero>
Candidate A ctx.tick(): <first> -> <increasing> | last player <id> | no sleep, no InputV1 mutation
```

### PASS criteria

All of the following should be true:

1. Game launches normally and reaches the watched match.
2. Candidate A enters once and is active during the early visible match, as in prior tests.
3. `AI OBSERVER total` increases.
4. `AI OBSERVER Candidate A` increases while Candidate A is active.
5. The observer thread id matches Candidate A's displayed worker thread id.
6. `Candidate A ctx.tick()` increases monotonically to a value far ahead of the early visible clock.
7. Candidate-A observer count/tick stop advancing when Candidate A completes.
8. Visible playback continues normally after Candidate A completes.
9. Champion behavior is unchanged from vanilla; no manual movement or idle override occurs.

### FAIL signals

Stop and report the overlay/log if any of these occur:

- launch crash;
- mod panic/disable;
- Candidate A is active but its observer count remains zero;
- observer Candidate-A callbacks appear on a different thread id;
- `ctx.tick()` is static, decreases materially, or behaves like an unrelated simulation;
- match behavior changes despite the observer returning `base_input` unchanged.

If the mod is disabled by a panic, inspect:

```text
%APPDATA%\TeamSamoyed\TeamfightManager2\data\log.log
```

Do not proceed to pacing until Stage 1 passes.

## Stage 2 — exact played-tick observation

Status: **not implemented yet**.

After Stage 1 passes, revalidate the statically traced match-view played tick at `match_view + 0x250`. The camera-containing object is already traced as `match_view + 0x960`, so the known-live camera capture can provide the owning match-view address without a new global hook.

This stage remains read-only. Its purpose is to compare authoritative Candidate-A `ctx.tick()` directly with the exact presentation tick instead of the coarse visible `MM:SS` string.

## Stage 3 — bounded pacing

Status: **not implemented yet**.

Only after Stages 1 and 2 pass, add cooperative waiting on one designated Candidate-A callback per simulation tick. Initial rule:

```text
if simulation_tick > played_tick + allowed_lead:
    wait briefly and re-check
```

Start with a conservative lead window and a bounded wait/fail-open path so the worker can never deadlock the match if presentation state disappears.

No manual `InputV1` should be reconnected until bounded pacing is physically stable.
