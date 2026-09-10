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

## Stage 2A — direct `match_view + 0x250` played-tick hypothesis

Status: **FAIL — physically rejected 2026-09-10**.

Four screenshots at visible `00:02`, `00:05`, `00:20`, and `00:30` showed the value at `match_view + 0x250` remained exactly `1`. Therefore it is not the advancing presentation tick for the live object and must not be used for pacing.

## Stage 2B — adjacent playback accumulator decode

Status: **FAIL — physically rejected 2026-09-10**.

The bounded decoder for `match_view + 0x258/+0x260` tested integer ticks, ms/us/ns, `f32/f64` seconds or ticks, and Rust-style `Duration`. At visible `00:05` and `00:10` it failed closed with `played tick --`, so none of those interpretations tracked presentation closely enough.

## Stage 2C — ranked match-view clock scan

Status: **FAIL — physically rejected 2026-09-10**.

A read-only scan of `match_view + 0x000 .. +0x95C` ranked common scalar time interpretations against the visible clock. At visible `00:05`, `00:10`, and `00:15`, the highest-ranked fields were still static zero words with very large and increasing errors. No useful simple presentation clock was found in the direct match-view prefix.

Further offset archaeology is deferred because Stage 3A proved it is not required to test real-time simulation pacing.

## Stage 3A — bounded 60 Hz Candidate-A pacing

Status: **PASS — physically validated 2026-09-10**.

This experiment paced only the confirmed Candidate-A worker from its StablePlayerAi callback while returning `base_input` unchanged. The pacer used the first post-InGame Candidate-A tick as its origin, held simulation to 60 ticks per monotonic wall-clock second with ~35 ms lead allowance, and intentionally auto-released after 30 seconds.

Representative physical results:

```text
visible 00:15 | origin 481 | tick 1416 | elapsed 15547 ms | released no
visible 00:20 | origin 481 | tick 1725 | elapsed 20687 ms | released no
visible 00:25 | origin 481 | tick 2031 | elapsed 25797 ms | released no
visible 00:30 | origin 481 | tick 6329 | elapsed 30547 ms | released YES
```

Before release:

- at 15.547 s, expected paced delta is ~932.8 ticks; observed delta was `1416 - 481 = 935`;
- at 20.687 s, expected paced delta is ~1241.2 ticks; observed delta was `1725 - 481 = 1244`;
- at 25.797 s, expected paced delta is ~1547.8 ticks; observed delta was `2031 - 481 = 1550`.

This is a very close match to 60 Hz. Candidate A remained `active 1` through the bounded pacing window. At the designed 30-second auto-release, the tick immediately began racing ahead again, eventually completing normally. Visible playback showed no strange behavior and vanilla AI input remained unchanged.

The user also exercised playback-speed controls and observed no obvious instability. Those controls were not yet synchronized to the pacer; after the 30-second release they only affected replay/presentation as usual. Continuous direct-control mode should initially be validated at 1x before pause/speed integration is attempted.

## Stage 3B — continuous 60 Hz full-match pacing

Status: **implemented; awaiting physical validation**.

Stage 3B removes only the artificial 30-second auto-release. Candidate A remains paced at fixed 60 Hz until the simulation naturally ends or a safety condition triggers.

Remaining safety behavior:

- only the confirmed Candidate-A worker is delayed;
- `base_input` remains untouched;
- ~35 ms lead allowance remains;
- waits occur in 1-2 ms slices;
- if one callback ever requires 250 ms of waiting, the pacer permanently fails open rather than risk stranding the worker.

The overlay now labels the release flag as `fail-open`. During a normal 1x full-match test it should remain `no`. Candidate A should remain `active 1` for the duration required to generate the match and become `active 0 / done 1` only when its simulation has naturally reached the end.

This test is intentionally fixed at 1x wall-clock pacing. Pause and playback-speed synchronization are separate follow-up work.

## Next step after Stage 3B

If continuous pacing passes, the next architectural test is no longer another pacing experiment. It is to reconnect a minimal manual `InputV1` for one selected player and measure the delay between a visible user command and the resulting visible action. That will reveal the actual producer/consumer lead between the live Candidate-A simulation and playback and tell us how much buffering must be removed or compensated for direct control.

## Parallel branch note

`feat/cursor-entity-picking` was created from the Stage-1 pacing branch tip. Its substantive work remains isolated in `src/entity_picker.rs` and `docs/entity-picking.md`, with a small wiring change in `src/lib.rs`. The pacing branch has since diverged substantially, so `src/lib.rs` should be expected to require a small manual merge resolution later, but no architectural conflict is expected.
