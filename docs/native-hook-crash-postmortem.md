# Native runner-loop hook crash postmortem

## Incident

A temporary `feat/mouse-diagnostic` build attempted to count entries at the common game-core runner loop head at VA `0x1418147F4` / RVA `0x018147F4`. Physical testing produced three repeatable process terminations during game launch. The user could not reach the game UI.

The build was rolled back immediately. `src/loop_probe.rs` was deleted and `src/lib.rs` no longer references or installs that hook.

## Root cause

The patch site was invalid for a 16-byte detour because the overwritten region had more than one legal entry point.

The intended loop head begins at:

```text
0x1418147F4  mov byte ptr [r13+0x2089], 0
0x1418147FC  cmp byte ptr [r13+0x2088], 0
```

The temporary probe overwrote both instructions (`0x1418147F4..0x141814803`) with an absolute-jump detour.

However, the original runner also contains this earlier conditional branch:

```text
0x14181477E  je  0x1418147FC
```

That branch intentionally skips the first instruction and enters at the second instruction. After the 16-byte detour was installed, `0x1418147FC` was no longer the original `cmp`; it was inside the detour's encoded jump/pointer bytes. When that branch was taken, execution entered data as if it were code, causing the repeatable crash.

This is sufficient to explain the launch failure; the earlier race/shared-hot-loop hypothesis is no longer needed as the primary explanation.

## Lessons / constraints

- Never choose a detour span solely because the displaced instructions themselves are relocatable.
- Before overwriting a multi-instruction span, verify that no branch/call/exception target enters the middle of that span.
- The runner loop at `0x1418147F4` may still be semantically useful, but a 16-byte entry detour there is forbidden.
- Prefer Candidate-A-specific instrumentation or a branch/call-site probe that does not destroy alternate control-flow entries.
- After any native crash, return to the last known-good hook set before adding another experiment.

## Current safe state

The retained native hooks are the previously validated camera hook and Candidate A/B/C simulation-task entry probes. `StablePlayerAi` remains disabled in this diagnostic build. No runner-loop hook is installed.
