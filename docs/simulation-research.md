# TFM2 v0.5.8 Client Simulation Research

This document records the reverse-engineering work around Teamfight Manager 2's client-side match simulation and playback timing. Camera research is in `camera-research.md`; screen-to-world validation is in `camera-validation-log.md`; physical direct-control tests are in `control-validation-log.md`.

## Tested executable

All native RVAs below apply only to the tested Teamfight Manager 2 v0.5.8 executable:

- file size: `77,666,816` bytes
- SHA-256: `4ed3aed08971efd06b7415817c9da9a444b6c7f63dc2ec09540b572568a4e045`
- PE timestamp: `0x6A978218`
- PE image size: `0x04A1D000`

Every runtime hook must verify the PE fingerprint and its target instruction bytes before patching memory.

## Why StablePlayerAi was not enough

Physical testing proved that `StablePlayerAi::think()` is invoked in enormous numbers of simulation/presimulation callbacks rather than once per player per visible frame. A temporary clock-gated experiment accepted `Unknown`-origin callbacks whose `ctx.tick()/60` matched the visible match clock. Manual `InputV1::move_to()` returns then increased, proving the input was reaching matching simulation ticks, but the watched champion still did not respond.

The decisive observation was that all AI counters eventually stopped while the visible match clock continued advancing. This means the relevant simulation work can finish ahead of presentation and the client can continue playing an already-computed result afterward. The StablePlayerAi clock gate is therefore diagnostic-only and is not the final interactive-control mechanism.

## Server/precomputed result versus watched live result

The v0.5.8 executable contains explicit diagnostics in `game-view/src/logic/server/packet_handler.rs` describing two results:

- a precomputed/server result;
- a separate live result corresponding to what was watched.

Relevant embedded diagnostics include:

- `[GamePlayDone] overriding precomputed result: ... server_blue_win=... live_blue_win=...`
- `[GamePlayDone] server/live simulation diverged on set ...`
- `Stored statistics and replay stay on the server run, so records and replay will not match what was watched.`

This is important evidence that the client has a local simulation path distinct from the stored server/precomputed result.

The GamePlayDone packet-handler function containing those diagnostics is approximately:

- VA `0x141A9FC70` through `0x141AA0CA1`

## Match-view playback state

Static tracing of the same match-view object already used for the camera hook found playback-related fields:

- `+0x250`: displayed/played match tick
- `+0x258`: playback elapsed-time accumulator

The client derives the played tick from the playback accumulator and consumes/render events corresponding to that point in the match.

Working model:

```text
client simulation runs ahead
        |
        v
result / frame-event stream
        |
        v
match-view playback
        |
        v
played tick (+0x250)
        |
        v
what the user sees
```

The direct-control problem is therefore a simulation-timing/ownership problem, not a camera or mouse-projection problem.

## game-core simulation runner

The executable retains the Rust source path:

`game-core/src/simulation/game/runner.rs`

A large runner function referencing many source-location records from that file is:

- VA `0x141813FB0` - `0x141818416`
- RVA `0x01813FB0` - `0x01818416`

Only one direct caller was found for that large body:

- call site `0x14180FE6C`
- containing wrapper function VA `0x14180EAA0` - `0x14180FFC9`

The wrapper at `0x14180EAA0` is called from many systems, including several client-side functions in `game-view/src/logic/client/data.rs`.

The first 32 bytes of both wrapper and runner were verified at runtime. Both start with the same eight-register push sequence, followed by their respective stack allocations. This independently matches the static disassembly used below.

## Client-side simulation candidates

Three client data functions were identified because they call the common game-core wrapper and are themselves invoked through closure/task-like wrappers.

### Candidate A — confirmed watched-match simulation job

Client data function:

- RVA `0x00B1CF10`
- VA `0x140B1CF10`
- pdata extent approximately `0x140B1CF10` - `0x140B1D719`
- source-location references around `game-view/src/logic/client/data.rs:6143-6149`
- game-core wrapper call at `0x140B1D052`

Closure/task wrapper:

- RVA `0x00B4B490`
- calls Candidate A at `0x140B4B559`
- captured context passed only in `RCX`

Physical probe results across two normal watched matches:

- Match 1: A entered once, was already active at visible `00:00`, and completed once after `7110 ms` wall-clock time. At visible `00:23`, the job was already done while playback continued normally.
- Match 2: A entered a second time for the second match, was still active at visible `00:05`, and completed by visible `00:08`; measured runtime was `6922 ms`.
- The job ran on a background thread (thread ids differed between matches, as expected for a task-pool worker).
- The captured context pointer also differed between matches, consistent with one per-match job object.

Conclusion: Candidate A is the client-side job that computes the watched match ahead of presentation. A full several-minute match is simulated in roughly seven seconds of wall-clock time, then the result continues to play back at presentation speed.

This confirms the earlier StablePlayerAi counter freeze: the authoritative client-side simulation can finish only a few visible seconds after match start.

### Candidate B — not observed in normal watched matches

Client data function:

- RVA `0x00B1DB20`
- VA `0x140B1DB20`
- source-location references around `game-view/src/logic/client/data.rs:6560-6567`
- game-core wrapper call at `0x140B1DC62`

Across two normal watched-match tests, B had zero entries.

### Candidate C — not observed in normal watched matches

Client data function:

- RVA `0x00B1E730`
- VA `0x140B1E730`
- source-location references around `game-view/src/logic/client/data.rs:4053-4054`
- game-core wrapper call at `0x140B1E843`

Across two normal watched-match tests, C had zero entries.

## Verified Candidate A/B/C hook prologues

All three client data candidates begin with the same 12 bytes:

```text
55 41 57 41 56 41 55 41 54 56 57 53
```

These are eight complete push instructions and contain no RIP-relative addressing. The Candidate A/B/C entry/exit probe has run successfully and is the current known-safe native simulation probe.

## Failed dominant-loop hook experiment — do not reuse

Static disassembly found a large backward edge inside the common runner:

- back-edge source: VA `0x1418175C8`
- target: VA `0x1418147F4`
- target RVA: `0x018147F4`

A temporary build attempted to detour 16 bytes at that loop head with a generated counter stub. The target bytes matched the expected v0.5.8 signature, but physical testing produced **three repeatable crashes during game launch**, before the user could enter the game.

Result: **this runtime patch strategy is rejected.** The temporary `src/loop_probe.rs` implementation was removed from the branch and the hook is no longer installed.

The crash mechanism has not been proven. The leading concern is that this address is a shared, very hot game-core loop used by simulation work outside Candidate A as well. Installing a multi-byte detour at mod initialization can race with another worker already executing that shared code, or can expose the probe to startup simulation contexts we did not intend to instrument. Do not assume the static loop target itself is semantically wrong; the unsafe part may be the global runtime-patching strategy.

## Revised next target

Do not patch the shared runner loop globally again.

Candidate A is already isolated and has a known call site into the common wrapper at `0x140B1D052`. Future dynamic instrumentation should be scoped through Candidate A or its call path so unrelated startup simulations cannot hit it. Preferred approaches, in order:

1. continue static disassembly inside Candidate A/common runner to identify a call-site or state field that exposes tick progress without patching the shared hot loop;
2. instrument a Candidate-A-specific call site rather than the global runner body;
3. if a shared inner function must eventually be observed, activate any instrumentation only for the confirmed Candidate A worker/context and use a patching mechanism that cannot race an executing instruction stream.

The long-term implementation directions remain:

1. pace the client live simulation so it stays only a small number of ticks ahead of presentation, allowing current client input to affect near-future decisions;
2. inject manual input directly at the native player-input decision point of Candidate A's simulation.

The first option is preferable only if pacing can be isolated to Candidate A's background worker without blocking render/UI or unrelated simulation jobs.
