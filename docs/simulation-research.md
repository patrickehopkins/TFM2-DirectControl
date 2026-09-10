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

## Client-side simulation candidates

Three client data functions are especially strong candidates for background simulation jobs because they call the common game-core wrapper and are themselves invoked through closure/task-like wrappers that copy captured state, run the simulation function, and publish a result into shared task storage.

### Candidate A

Client data function:

- RVA `0x00B1CF10`
- VA `0x140B1CF10`
- pdata extent approximately `0x140B1CF10` - `0x140B1D719`
- source-location references around `game-view/src/logic/client/data.rs:6143-6149`
- game-core wrapper call at `0x140B1D052`

This function contains an internal loop that returns to the simulation call path, so it may process multiple sets/simulations within one job.

Closure/task wrapper:

- RVA `0x00B4B490`
- calls Candidate A at `0x140B4B559`
- captured context passed only in `RCX`

### Candidate B

Client data function:

- RVA `0x00B1DB20`
- VA `0x140B1DB20`
- pdata extent approximately `0x140B1DB20` - `0x140B1E329`
- source-location references around `game-view/src/logic/client/data.rs:6560-6567`
- game-core wrapper call at `0x140B1DC62`

Closure/task wrapper:

- RVA `0x00B4B7B0`
- calls Candidate B at `0x140B4B851`
- captured context passed only in `RCX`

### Candidate C

Client data function:

- RVA `0x00B1E730`
- VA `0x140B1E730`
- pdata extent approximately `0x140B1E730` - `0x140B1EDD7`
- source-location references around `game-view/src/logic/client/data.rs:4053-4054`
- game-core wrapper call at `0x140B1E843`

Closure/task wrapper:

- RVA `0x00B4BA90`
- calls Candidate C at `0x140B4BB68`
- captured context passed only in `RCX`

## Verified hook prologues

All three client data candidates begin with the same 12 bytes:

```text
55 41 57 41 56 41 55 41 54 56 57 53
```

These are eight complete push instructions:

```text
push rbp
push r15
push r14
push r13
push r12
push rsi
push rdi
push rbx
```

There is no RIP-relative instruction in this 12-byte window, so it can be copied safely into the same style of trampoline already validated by the camera probe.

The closure wrappers set `RCX` to one captured context pointer immediately before calling these functions and do not consume a return value. The diagnostic hooks therefore use a one-argument `extern "system" fn(*mut u8)` ABI and call the originals unchanged through trampolines.

## Native simulation-task probe

`src/simulation_probe.rs` instruments Candidate A/B/C at mod initialization, before a match can begin. It is read-only with respect to simulation/gameplay state. For each candidate it records:

- entry count;
- currently active call count;
- completion count;
- last Windows thread ID;
- last captured context pointer;
- last elapsed duration;
- maximum observed duration.

The StablePlayerAi direct-control override is intentionally disabled in this probe build so millions of AI diagnostic callbacks cannot perturb the timing being measured.

The purpose of the first physical test is to identify which candidate is associated with a normal watched match. A strong live-simulation candidate would typically enter around match preparation/start, run on a background thread, and either remain active while its simulation runs or complete before/while visible playback begins.

## Next step after candidate identification

Do not pace or mutate the simulation until the relevant job is identified. Once the normal watched-match job is known, trace that candidate's simulation loop and determine the narrowest safe pacing point relative to the match-view played tick.

Two implementation directions remain plausible:

1. Pace the client live simulation so it stays only a small number of ticks ahead of presentation, allowing current client input to affect near-future simulation decisions.
2. Inject manual input directly at the native player-input decision point of the identified client live simulation.

The first option is preferable if the live simulation can be paced without blocking the render/UI thread or other unrelated background simulations.
