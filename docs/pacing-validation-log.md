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

## Stage 2A — direct `match_view + 0x250` played-tick hypothesis

Status: **FAIL — physically rejected 2026-09-10**.

The first Stage-2 build derived `match_view = camera_address - 0x960` from the known-live camera subobject and read an integer at `match_view + 0x250`, based on the earlier static interpretation that this was the currently played tick.

Four screenshots from one normal watched match showed:

```text
visible 00:02 | value at +0x250 = 1 | Candidate A ctx.tick() = 19905
visible 00:05 | value at +0x250 = 1 | Candidate A ctx.tick() = 40807
visible 00:20 | value at +0x250 = 1 | Candidate A already complete
visible 00:30 | value at +0x250 = 1 | Candidate A already complete
```

The derived match-view address remained stable and the camera hook continued receiving calls, but `+0x250` remained exactly `1` throughout. Therefore `+0x250` is **not** the live advancing presentation tick for this object. Do not use it for pacing.

This failure does not invalidate Candidate A or the camera-to-owner relationship; it only rejects the field interpretation.

## Stage 2B — adjacent playback accumulator decode

Status: **FAIL — physically rejected 2026-09-10**.

The next probe targeted the earlier statically suspected `match_view + 0x258/+0x260` playback accumulator and evaluated common native time representations (`u32/u64` ticks, ms/us/ns, `f32/f64` seconds or ticks, and Rust-style `Duration`). It was designed to fail closed if no interpretation tracked early 1x playback closely enough.

Physical screenshots at visible `00:05` and `00:10` both showed:

```text
played tick -- | match-view --
```

while Candidate A and the rest of the match continued normally. Therefore none of the bounded interpretations of `+0x258/+0x260` matched presentation strongly enough to pass the decoder threshold. Do not use these offsets for pacing.

## Stage 2C — ranked match-view clock scan

Status: **implemented; awaiting physical validation**.

Instead of guessing individual offsets, the current probe scans the entire known-live match-view prefix before the embedded camera object (`match_view + 0x000 .. +0x95C`, with the camera beginning at `+0x960`). It reads aligned 32-bit words and evaluates common scalar time interpretations at each offset, including adjacent 64-bit representations.

For each offset, only its best interpretation is retained. The overlay then shows the strongest three candidates ranked by how closely their delta matches:

```text
visible elapsed seconds * 60 ticks/second
```

at normal 1x playback. Static/nearly-static fields are heavily penalized once enough visible time has elapsed, and fields with implausible absolute values are rejected.

Expected overlay form:

```text
CLOCK #1: +0xXYZ <encoding> | ticks ... | delta ... | error ...
CLOCK #2: ...
CLOCK #3: ...
```

A useful candidate should converge toward a low error as the visible clock advances. One screenshot around 00:05 and another around 00:10-00:15 should be enough to determine whether a stable offset/encoding is emerging. A candidate must still pass a later pause/speed test before it can be trusted for pacing.

This stage remains read-only. No sleeping, pacing, player selection, or manual `InputV1` is enabled.

## Stage 3 — bounded pacing

Status: **not implemented yet**.

Only after a presentation clock source passes Stage 2, add cooperative waiting on one designated Candidate-A callback per simulation tick. Initial rule:

```text
if simulation_tick > played_tick + allowed_lead:
    wait briefly and re-check
```

Start with a conservative lead window and a bounded wait/fail-open path so the worker can never deadlock the match if presentation state disappears.

No manual `InputV1` should be reconnected until bounded pacing is physically stable.

## Parallel branch note

`feat/cursor-entity-picking` was created from the Stage-1 pacing branch tip and therefore already contains the Stage-1 observer. Its substantive work is isolated in `src/entity_picker.rs` and `docs/entity-picking.md`; it also has a small wiring change in `src/lib.rs`. The pacing branch has since diverged. Future pacing work should remain modular. A later merge may require a small manual resolution in `src/lib.rs`, but no architectural conflict is currently expected.
