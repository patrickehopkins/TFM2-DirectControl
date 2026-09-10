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

Status: **FAIL — no useful direct scalar presentation clock found in scanned prefix, physically tested 2026-09-10**.

The probe scanned the entire known-live match-view prefix before the embedded camera object (`match_view + 0x000 .. +0x95C`) and evaluated common 32-bit and adjacent 64-bit scalar time interpretations.

Screenshots at visible `00:05`, `00:10`, and `00:15` showed that the top-ranked fields remained static zero-valued words at `+0x000`, `+0x004`, and `+0x008`, with very large and worsening errors:

```text
00:05 -> error ~10300 ticks
00:10 -> error ~10600 ticks
00:15 -> error ~10900 ticks
```

No stable field/encoding converged toward the expected `visible_elapsed * 60` relationship. This does not prove presentation time is absent from all client state; it does show that continued blind scalar-offset hunting in this known-live prefix is low-value compared with testing direct simulation pacing.

The clock scanner is therefore retired from the active build. Its source remains in repository history as research evidence.

## Stage 3A — bounded 60 Hz wall-clock pacing proof

Status: **implemented; awaiting physical validation**.

This experiment tests the simpler direct hypothesis: pace Candidate A itself at approximately 60 simulation ticks per monotonic wall-clock second, without requiring a discovered replay/presentation clock.

Implementation boundaries:

- only `StablePlayerAi::think()` callbacks executing on the confirmed Candidate-A worker are delayed;
- the pacer records a simulation-tick origin and monotonic wall-clock origin;
- the mod re-anchors pacing when the visible client first enters `InGame`;
- Candidate A is allowed about `35 ms` of simulation lead to absorb scheduler/sleep jitter;
- waits occur in small bounded sleep slices;
- any single callback that would need more than `250 ms` of waiting fails open;
- after `30 seconds` of wall-clock pacing the experiment automatically releases and Candidate A is allowed to finish at normal full speed;
- `base_input` is returned unchanged: no manual player input is emitted.

Expected behavior during the first 30 seconds:

```text
Candidate A: active 1
ctx.tick delta ~= elapsed wall seconds * 60
PACER released: no
wait loops / slept ms: increasing
```

At roughly 30 seconds:

```text
PACER released: YES
Candidate A rapidly completes
```

The strongest success signal is that Candidate A remains active for tens of seconds instead of completing in roughly 4-7 seconds, while its simulation tick advances at approximately real-time speed and visible playback continues.

If the visible match never starts, freezes, or the game/mod crashes, stop and report the symptom. That would mean pacing Candidate A this early interferes with the producer/consumer startup assumptions and the pacing origin/window will need adjustment.

No manual `InputV1` should be reconnected until bounded pacing is physically stable.

## Parallel branch note

`feat/cursor-entity-picking` was created from the Stage-1 pacing branch tip and therefore already contains the Stage-1 observer. Its substantive work is isolated in `src/entity_picker.rs` and `docs/entity-picking.md`; it also has a small wiring change in `src/lib.rs`. The pacing branch has since diverged. Future pacing work remains modular. A later merge may require a small manual resolution in `src/lib.rs`, but no architectural conflict is currently expected.
