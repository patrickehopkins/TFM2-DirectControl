# Replay shortcut suppression — v0.6.1 native analysis

Status: **release blocker unresolved**. The original UI hiding works, but remappable shortcuts
can still move the visible replay away from Candidate A's live simulation. Never describe the
UI cleanup as a full rewind disable.

## Inspected executable

- File: `TeamfightManager2.exe`, 86,330,880 bytes (supplied by the user)
- SHA-256: `91084e9a29c70993595a1d7d0c22064bae0ee82b0fc773c696077d15c2268f98`
- PE timestamp: `0x6AB1D950`
- PE image size: `0x05264000`
- This matches the pre-existing, verified v0.6.1 camera-hook build profile.

All addresses below are **module-relative RVAs**, not absolute pointers. Do not reuse them
for a different executable without re-verifying both the build and instructions.

## Confirmed semantic action identifiers

The native action-name conversion function at RVA `0x0215D0C0` indexes a
`u8` variant number into a jump table and produces the corresponding native action name.
The relevant mapping is:

| Native action variant | Action |
| --- | --- |
| `0x1B` (27) | `in_game_highlight_mode` |
| `0x30` (48) | `in_game_prev_highlight` |
| `0x31` (49) | `in_game_prev_time` |
| `0x32` (50) | `in_game_pause_time` |
| `0x33` (51) | `in_game_next_time` |
| `0x34` (52) | `in_game_next_highlight` |
| `0x35` (53) | `in_game_zoom_in` |
| `0x36` (54) | `in_game_zoom_out` |

The associated string-descriptor table covers the replay actions at
`0x03A94270..0x03A942D0`.

The existing v0.6.1 native camera handler at RVA `0x00C2DBE0` reads a
55-entry `0..54` action list at `0x03BF2986`. In an initialization path it maps
each action through `0x021DBD30` into a default keyboard binding, then inserts
that pair using `0x00CEB890`. **This is configuration evidence, not proof that
the same functions execute/reject actions during live playback.**

## Sites that must NOT be mistaken for dispatch

- `0x0215D0C0` formats or names action variants; hooking it does not reject them.
- `0x00CC4500` and nearby functions reference the action descriptor table while
  handling configuration/serialization. Their presence is not evidence of action execution.
- `0x00CEB890` inserts action/default-key pairs; altering defaults would fail for user
  remapped shortcuts already loaded in memory.
- The camera hook's initialization path is not yet proven to own the runtime
  shortcut state or the replay-position mutator.
- A Win32 physical-key filter (`M`, `6`, etc.) would be defeated by rebinding and
  should not be shipped as semantic seek suppression.

## Required implementation contract

Identify the **runtime** shortcut-state query/action dispatcher or the authoritative
replay seek mutator. Intercept *semantic actions* after remapping, not their default keycodes.
Under guarded v0.6.1 build detection and a verified hook prologue:

1. While the current watched match is controlled, block previous/next time,
   previous/next highlight, and highlight playback mode no matter the assigned
   keyboard shortcut or UI route. Preserve safe, coordinated pause behavior.
2. Preserve the validated camera control path (MMB drag and wheel zoom), so
   action names for zoom are **not** in the forbidden native-action list.
3. Stop blocking only upon the *confirmed* `Ctrl+End` release for that match.
   `End` temporary yield must not silently re-enable seeking while Candidate A
   is still live-paced.
4. Do not enable any new functionality in multiplayer or break the locked 1x
   multiplayer invariant.
5. On unknown signatures or hook failures, **report the missing protection**
   clearly rather than claiming rewind is disabled.

Verify on an actual match by rebinding the target shortcuts, attempting each
blocked action through keyboard/UI, and confirming native behavior returns
after confirmed `Ctrl+End`. The author should also retest the ghost zoom-tooltip
cleanup and ordinary camera zoom.

## What analysis cannot yet establish

The supplied executable is stripped native Rust. Static references identify
action names and default-binding construction, but so far **do not establish a
safely hookable action dispatcher or seek setter with a verified calling
convention**. Do not install a guessed native detour merely from an xref cluster.
If static control-flow analysis remains ambiguous, collect a narrowly scoped
runtime trace from a known action invocation on this same executable.

## Verified generic action hash and opt-in native runtime trace

Offline inspection of the *user-supplied* v0.6.1 executable further identified
RVA `0x00BA3A30` as a generic single-byte key/action hasher. Its first 13
bytes are exactly:

```text
48 83 EC 78              sub rsp, 78h
F3 0F 6F 01              movdqu xmm0, [rcx]
66 0F 70 C8 44           pshufd xmm1, xmm0, 44h
```

Its observed calling convention is `hash(hasher_ptr, one_byte_key_ptr) -> u64`.
**Importantly, this function hashes keys in other paths too**; merely observing
an action-like byte in this hook is not sufficient to prove that a replay action
was dispatched. The caller RVA and experimental test state are required context.

Five native UI replay button callbacks near `0x009B15D0..0x009B1BD0`
hash their corresponding configured shortcut identifiers and synthesize normal
keyboard messages. A separate large event-processing function near
`0x0083DB00` consumes tagged key messages and queries an in-memory shortcut
map. The ultimate semantic seek mutator and a safe native rejection site
**remain unverified**.

An intentionally temporary feature-gated diagnostic now lives in
`src/replay_native_trace.rs`; it **patches process code** to detour the
generic hasher but forwards calls unchanged, without intentionally modifying
simulation or input. Its install is guarded by the exact v0.6.1 PE header and
all 13 prologue bytes. It captures only the interesting one-byte values and
bounded call-stack samples, with a separate quota for each experiment.

### One controlled game-side trace

This instrumented build has not been compiled or executed by the assistant:
it must receive a Windows physical test before its findings can be trusted.
Close TFM2 before installing:

```powershell
git pull
.\scripts\install-dev.ps1 -ReplayTrace
```

Use a disposable/practice match. Confirm `Ctrl+End` global release *before*
any rewind tests; this preserves the currently validated live-control behavior.
While ordinary replay controls are restored, press `Ctrl+Alt+F12` once to begin
capture. Then click **Back 10 Seconds**, press the user's bound `M` for the
same action, and press `6` for Previous Highlight, with distinct deliberate
key holds. Press `Ctrl+Alt+F12` again to write
`%TEMP%\tfm2_replay_native_trace.txt`. Stop and report if the game becomes
unstable; revert to the ordinary build with
`.\scripts\install-dev.ps1` (no feature switch).

Interpretation: call stacks from the UI and rebound keyboard actions can
identify a common shortcut processing path, or reveal that the runtime input
path does **not** rehash semantic actions. No trace hits is a useful negative
finding, not permission to pretend the actions were suppressed.

This diagnostic must be removed or remain disabled for Workshop packaging.
The real release blocker closes only after a *separate* native-action-level
suppression implementation is physically verified.
