# Candidate A pacing validation log

This file records physical validation of the staged pacing work for Teamfight Manager 2 v0.5.8.

## Stage 1 — read-only StablePlayerAi observer

Status: **PASS — physically validated 2026-09-10**.

Two screenshots from one normal watched match proved that StablePlayerAi callbacks on Candidate A's worker expose the watched simulation tick. Candidate A raced hundreds of simulated seconds ahead of presentation when left unpaced, all ten player ids were observed (`players mask 0x3FF`), and the observer thread id exactly matched Candidate A's worker thread id. Input remained vanilla AI.

## Stage 2A — direct `match_view + 0x250` played-tick hypothesis

Status: **FAIL — physically rejected 2026-09-10**.

Four screenshots at visible `00:02`, `00:05`, `00:20`, and `00:30` showed the value at `match_view + 0x250` remained exactly `1`. Therefore it is not the advancing presentation tick for the live object and must not be used for pacing.

## Stage 2B — adjacent playback accumulator decode

Status: **FAIL — physically rejected 2026-09-10**.

The bounded decoder for `match_view + 0x258/+0x260` tested integer ticks, ms/us/ns, `f32/f64` seconds or ticks, and Rust-style `Duration`. At visible `00:05` and `00:10` it failed closed with `played tick --`, so none of those interpretations tracked presentation closely enough.

## Stage 2C — ranked match-view clock scan

Status: **FAIL — physically rejected 2026-09-10**.

A read-only scan of `match_view + 0x000 .. +0x95C` ranked common scalar time interpretations against the visible clock. At visible `00:05`, `00:10`, and `00:15`, the highest-ranked fields were still static zero words with very large and increasing errors. No useful simple presentation clock was found in the direct match-view prefix.

Further offset archaeology is deferred because Stage 3 proved it is not required for real-time simulation pacing.

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

Before release, the observed simulation tick stayed within only a few ticks of ideal 60 Hz pacing. At the designed 30-second auto-release, the tick immediately began racing ahead again and eventually completed normally. Visible playback showed no strange behavior.

## Stage 3B — continuous 60 Hz full-match pacing

Status: **PASS — physically validated 2026-09-10**.

Stage 3B removed the artificial 30-second release and kept Candidate A paced continuously at fixed 60 Hz. The user ran the watched match to at least visible `10:00`; Candidate A remained `active 1 / done 0` throughout and the safety fail-open remained `no`.

Representative physical results:

```text
visible 01:20 | origin 10 | tick 4848  | elapsed 80594 ms  | fail-open no
visible 03:32 | origin 10 | tick 12772 | elapsed 212657 ms | fail-open no
visible 06:05 | origin 10 | tick 21912 | elapsed 364985 ms | fail-open no
visible 10:00 | origin 10 | tick 36039 | elapsed 600438 ms | fail-open no
```

Pacing accuracy remained extremely tight. At visible 10:00, Candidate A had advanced `36039 - 10 = 36029` ticks over 600.438 seconds; exact 60 Hz predicts ~36026 ticks, a lead of roughly three ticks (~50 ms). Earlier checkpoints show the same small bounded lead rather than accumulating drift.

The Candidate-A worker thread id remained `25328` for the whole observed match. This is expected: it is the Windows thread assigned to that one running simulation job. Previous matches used different worker ids, so a stable id within one match is evidence of continuity, not a stall.

### Playback-speed observation during continuous pacing

The game remembered a prior 3x playback setting, but while Candidate A was live and producer-limited to ~60 Hz, visible playback behaved at roughly 1x regardless of selecting 0.5x, 1x, 1.5x, 2x, or 3x. No instability was observed.

The current interpretation is that an unfinished watched simulation keeps presentation near the producer/live edge, so replay-speed controls cannot outrun or substantially lag the live producer. This is desirable for the v1 direct-control path: fixed real-time simulation naturally yields real-time visible play. User-selectable live simulation rates are explicitly deferred and can later be implemented by changing the pacer rate rather than relying on replay-speed controls.

## Stage 3C — irreversible manual release / finish simulation

Status: **implemented; awaiting physical validation**.

The user requested a way to stop direct control and let Teamfight Manager 2 finish computing the rest of the match immediately. This is intentionally one-way because once Candidate A is allowed to race ahead, returning to a meaningful live manual-control point is not supported.

Control:

```text
Ctrl+End -> permanently release direct control for this match and remove Candidate-A pacing
```

Design details:

- `Ctrl+End` was chosen as a deliberate, mnemonic chord and avoids the F1-F10 player-slot keys, F11 fullscreen, and F12/Steam screenshot conventions.
- The release latch resets only when a new InGame match begins.
- After release, Candidate A is allowed to race to natural completion.
- Future manual `InputV1` code must consult the same release latch and return vanilla AI input for the remainder of the match.
- The overlay explicitly says `CANNOT RESUME THIS MATCH`.
- Manual release and safety fail-open are tracked separately so diagnostics always show why pacing stopped.

Physical validation target: while Candidate A is still active, press Ctrl+End once. The overlay should change `manual finish no -> YES`, Candidate A's tick should immediately race ahead, and `active 1 / done 0` should shortly become `active 0 / done 1`. The visible replay should continue normally.

## Next step after Stage 3C

After the manual-release latch is validated, the next architectural test is to reconnect a minimal manual `InputV1` for one selected player and measure the delay between a visible user command and the resulting visible action. That will reveal the remaining producer/consumer lead and determine whether any small startup buffer needs to be reduced or compensated for direct control.

## Parallel branch note

`feat/cursor-entity-picking` was created from the Stage-1 pacing branch tip. Its substantive work remains isolated in `src/entity_picker.rs` and `docs/entity-picking.md`, with a small wiring change in `src/lib.rs`. The pacing branch has since diverged substantially, so `src/lib.rs` should be expected to require a small manual merge resolution later, but no architectural conflict is expected.
