# Teamfight Manager 2 v0.6.3 Migration Handoff — Harbinger Direct Control

Date: 2026-10-07

Status: **simulation Candidate A is now physically cross-validated on v0.6.3; replay-gate relocation remains strong static evidence; camera relocation is still required before a runnable Harbinger v0.6.3 build should be enabled.**

This document is intentionally fail-closed. Do not mark Harbinger compatible with v0.6.3 from static evidence alone.

## Exact executable analyzed

User-supplied `TeamfightManager2.exe`:

- SHA-256: `f21cf691799a83d9afa0ccae3e2b95862f5780446a501ee97860b91b752dbe2f`
- PE timestamp: `0x6AC5C567`
- PE image size: `0x052D6000`
- image base: `0x140000000`

All native RVAs in this handoff are specific to that fingerprint.

A Creep Chaos probe built from the user's installed v0.6.3 stable SDK loaded successfully and reported host ABI **9**, so the stable SDK ABI level remains compatible at the ABI-number level. Harbinger should still bootstrap/copy the installed v0.6.3 SDK rather than reuse an older local SDK tree.

## 1. Simulation job triple: strong static relocation

Current Harbinger v0.6.2 layout in `src/simulation_probe.rs`:

```text
timestamp  0x6ABC597E
image      0x052B8000
A          0x00BE42B0
B          0x00BE4D40
C          0x00BE57D0
```

Relocated v0.6.3 triple:

```text
timestamp  0x6AC5C567
image      0x052D6000
A          0x00BFC320
B          0x00BFCDB0
C          0x00BFD840
```

Evidence:

- each function moved by exactly `+0x18070`;
- A→B and B→C spacing remains exactly `0xA90`;
- all three retain Harbinger's detour-safe 12-byte prologue:
  `55 41 57 41 56 41 55 41 54 56 57 53`;
- v0.6.3 A and B are both `0x789` bytes and remain structural/direct-call twins;
- v0.6.3 C is `0x63B` bytes and retains the distinct third-job shape.

Recommended code addition:

```rust
const BUILD_0_6_3: SimulationLayout = SimulationLayout {
    pe_timestamp: 0x6AC5_C567,
    image_size: 0x052D_6000,
    candidate_rvas: [0x00BF_C320, 0x00BF_CDB0, 0x00BF_D840],
    core_wrapper_rva: None,
    core_runner_anchor_rva: None,
};
```

Add it to `known_layout`.

### Runtime confidence boundary — updated after Creep Chaos Probe 002

Creep Chaos Probe 002 has now physically exercised the relocated v0.6.3 Candidate-A detour.

The watched-match capture contains exactly one match start / one tick-1 initialization and then one coherent timeline through tick 38,290; the duplicate ClientMatchView sequence seen without Candidate-A filtering is gone.

Therefore **Candidate A at `0x00BFC320` is physically cross-validated as the watched/live client simulation job on the supplied v0.6.3 executable.**

This is strong evidence for Harbinger's simulation-layout migration, but it is not a substitute for Harbinger's own full Direct Control smoke test: pacing, ownership, control injection, UI safety, camera, and release behavior still need physical validation together.

## 2. Replay action lookup: strong static relocation

Current v0.6.2 `src/replay_action_gate.rs`:

```text
binding_lookup_rva = 0x028D3E90
```

v0.6.3 relocation:

```text
binding_lookup_rva = 0x01C33180
```

The existing exact 12-byte prologue:

```text
56 53 48 83 EC 28 89 D3 88 54 24 27
```

occurs **once** in the supplied v0.6.3 executable. Disassembly of that unique function still shows the action-byte/key-binding lookup shape and fallback mapping behavior expected by the current hook.

Recommended code addition:

```rust
const BUILD_0_6_3: ReplayActionLayout = ReplayActionLayout {
    pe_timestamp: 0x6AC5_C567,
    image_size: 0x052D_6000,
    binding_lookup_rva: 0x01C3_3180,
};
```

Add it to `known_layout`.

Keep the existing exact prologue verification. Do not derive this RVA by applying the simulation delta; the replay function moved independently.

## 3. Camera hook: still a migration blocker

Harbinger has a third shipping native compatibility table in:

```text
src/camera_probe/base.rs
```

Current v0.6.2 layout:

```text
handler_rva              0x00CAF090
zoom_offset              0x110
center_x_offset          0x114
center_y_offset          0x118
extent_a_offset          0x11C
extent_b_offset          0x120
vision_object_offset     0x448
vision_mode_offset       0x63
vision_write_guard       0x10
pan_x_offset             0x458
pan_y_offset             0x45C
```

The camera handler uses the same generic 12-byte push prologue as many Rust functions. Unlike the simulation triple and replay lookup, the supplied v0.6.3 binary does **not** provide a unique enough signature from the currently documented evidence to safely promote one candidate and its private object offsets.

Therefore:

- do not reuse `0x00CAF090`;
- do not assume the v0.6.2 private offsets survived;
- do not add a guessed v0.6.3 `CameraLayout`;
- keep Harbinger fail-closed on v0.6.3 until the handler and all touched offsets are independently relocated.

This matters to shipping behavior, not merely diagnostics: MMB pan, wheel zoom, camera capture/world projection support, and automatic team fog depend on this native layout.

### Recommended camera relocation procedure

Resume from the proven method documented in `docs/camera-research.md`:

1. relocate the camera input/update handler structurally, not by nearest RVA;
2. verify the first 12 bytes are complete non-RIP-relative pushes;
3. identify the active camera object's zoom, center, extent, pan, and vision-owner accesses from the relocated handler;
4. keep `mode_offset = None` unless directly proven;
5. create a complete v0.6.3 `CameraLayout` only after all fields used by write paths are verified together;
6. physically validate MMB pan, wheel zoom, minimap relocation composition, and team-fog enforcement.

A partial layout is worse than leaving 0.6.3 unsupported because Harbinger writes pan/zoom/vision fields.

## 4. Opt-in replay native trace

`src/replay_native_trace.rs` is an explicitly diagnostic, feature-gated v0.6.1-only hook:

```text
PE_TIMESTAMP 0x6AB1D950
PE_IMAGE_SIZE 0x05264000
HASH_RVA      0x00BA3A30
```

It is not part of the normal Workshop build.

Do not treat failure of this diagnostic feature as a release blocker. If the trace is needed again on 0.6.3, relocate it separately. Never enable it in Workshop packaging.

## 5. Cross-project simulation identity finding

Creep Chaos Probe 001 exposed a point that reinforces Harbinger's architecture:

`SimOriginKindV1::ClientMatchView` alone is **not a unique live-simulation identity**. Multiple client simulation copies can share the same origin family, seed, entity IDs, and early tick sequence.

Harbinger's Candidate-A native job discriminator is therefore not redundant bookkeeping. It is currently the evidence-backed boundary that prevents external control/pacing from attaching to the wrong simulation copy.

Do not replace Candidate-A discrimination with:

- "highest tick";
- "first ClientMatchView";
- ignoring backward ticks;
- seed equality.

## 6. Files that must change for v0.6.3 support

Functional migration:

- `src/simulation_probe.rs` — add exact 0.6.3 layout above;
- `src/replay_action_gate.rs` — add exact 0.6.3 replay lookup above;
- `src/camera_probe/base.rs` — **blocked pending camera relocation**.

After physical validation:

- `README.md` compatibility/current target;
- `mod.mod_info` version/compatibility text;
- Workshop description/change note;
- any release checklist/status document that still says v0.6.2.

Do not change public compatibility metadata before camera relocation and physical tests pass.

## 7. Required physical validation

Once all three shipping native layouts are present, run the existing Harbinger smoke/regression suite on v0.6.3:

1. startup reaches READY without hanging;
2. Ctrl+Home starts the intended live watched simulation;
3. presentation circles/entities remain synchronized;
4. F1-F5 select the user's team correctly regardless of blue/red side;
5. F6-F10 select opponents;
6. contextual RMB ground movement;
7. contextual RMB exact-target attack;
8. held RMB sweep;
9. minimap movement/attack;
10. A attack-move;
11. Q/W/R position, direction, entity-target, and immediate self/cursorless paths;
12. H Hold;
13. B Return;
14. synchronized pause/resume;
15. MMB drag;
16. wheel zoom;
17. automatic team fog;
18. End temporary AI release;
19. Ctrl+End confirmation and permanent release;
20. replay seek/highlight/timeline shortcuts blocked while owned and restored after Ctrl+End.

Then retain the known regression checks:

- duplicate player names;
- remapped native follow shortcuts;
- hidden HUD;
- background-focus safety;
- no duplicate local + Workshop install during smoke test.

## 8. Version/release recommendation

Do not bump Workshop compatibility solely from this handoff.

A safe sequence is:

1. add simulation + replay 0.6.3 layouts on a migration branch;
2. relocate camera layout;
3. build against the installed v0.6.3 SDK;
4. run physical smoke/regression tests;
5. inspect `harbinger-diagnostics.log`;
6. only then update metadata and publish.

## Bottom line

Two of Harbinger's three shipping native compatibility surfaces have high-confidence v0.6.3 relocations:

- simulation job triple: **relocated**
- replay action lookup: **relocated**
- camera handler/private layout: **not yet safely relocated**

Until the camera item is resolved, Harbinger should continue to reject v0.6.3 rather than run partially migrated.


## 9. Cross-project stable match-hook execution finding

Creep Chaos Probe 004 added an important stable-SDK runtime observation relevant to any future Harbinger work that uses `StableMatchHook`.

In one ordinary v0.6.3 match, the hook's process-global counter exceeded **1,000,000 calls before Candidate-A watched tick 1** and continued climbing rapidly while the visible match advanced normally. Large numbers of otherwise-valid 10-champion / 16-tower simulation copies were observed.

This confirms in practice that `StableMatchHook` is invoked across the game's internal simulation workload, not once per visible/watched tick.

Implications for Harbinger:

- do not use process-global hook-call counts as a live-match clock;
- do not assume `ClientMatchView`, seed equality, or ordinary 5v5 entity counts identify the watched simulation;
- Candidate-A discrimination remains the evidence-backed watched-client boundary;
- any future gameplay mutation implemented through the stable match hook must explicitly decide whether it is intended to affect **all deterministic simulation copies** or only a specific observed/live context;
- external direct-control input/presentation logic should continue to attach to Candidate A rather than a generic stable match callback.

Creep Chaos used Candidate-A-only mutation in Probe 005 solely as an architecture proof so the effect could be correlated with the watched client. That restriction should **not** be copied into Harbinger gameplay logic without separately considering multiplayer determinism and AI/pre-sim consistency.
