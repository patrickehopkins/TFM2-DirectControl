# Candidate A pacing plan

This note records the safer pacing route discovered after the failed shared runner-loop detour. The goal is to avoid patching the hot game-core loop entirely.

## Candidate A to runner-state pointer chain

Static disassembly of Candidate A (`VA 0x140B1CF10`) shows:

```text
0x140B1CF4F  mov rsi, [rcx+0x28]
...
0x140B1CFE7  mov rax, [rbp+0x5A8]    ; saved Candidate A context
0x140B1CFEE  mov rbx, [rax+0x10]
...
0x140B1D034  lea rdx, [rbx+0x20]
...
0x140B1D052  call 0x14180EAA0        ; common game-core wrapper
```

Therefore for Candidate A's captured context pointer:

```text
shared       = *(context + 0x10)
runner_state = shared + 0x20
```

The common wrapper immediately preserves that argument as its runner state:

```text
0x14180EB12  mov r13, rdx
```

and later calls the large runner with that same pointer in `RCX`:

```text
0x14180FE65  mov rcx, [rbp+0x480]     ; previously saved r13
0x14180FE6C  call 0x141813FB0         ; game-core runner
```

So Candidate A already gives us the runner-state address; no inner-loop hook is required merely to locate it.

## Simulation tick accessor

The runner repeatedly uses a trait object at:

```text
runner_state + 0x1DC0 = trait data pointer
runner_state + 0x1DC8 = trait vtable pointer
```

Calls through vtable offset `+0x28` are used as the simulation-time/tick accessor. Three concrete vtables found in the v0.5.8 executable all share the same accessor implementation:

```text
VA 0x141851F10
    mov rax, [rcx+0xEC98]
    ret
```

The runner later converts this returned integer to float and divides it by a constant `60.0`, confirming that the value is the 60-Hz simulation tick/time counter used to derive seconds.

Thus the currently known state path is:

```text
runner_state = *(candidate_a_context + 0x10) + 0x20
trait_data   = *(runner_state + 0x1DC0)
sim_tick     = *(trait_data + 0xEC98)
```

This must still be validated at runtime before it is used for pacing, but it is strongly supported by the static code path.

## Safer StablePlayerAi isolation

The Candidate A entry hook already records the Windows worker-thread ID before it invokes the original Candidate A body. Earlier `StablePlayerAi` experiments were noisy because they accepted callbacks from many simulations/origins. A safer diagnostic is:

1. register `StablePlayerAi` for all ten players but return `None` unconditionally;
2. inside `think()`, compare `GetCurrentThreadId()` with the currently active Candidate A worker-thread ID;
3. record `ctx.tick()` only when the IDs match;
4. do not change player input in this validation build.

If those callbacks track Candidate A's runner tick, we have an authoritative AI-decision point without guessing `SimOriginKindV1`.

## Pacing concept

Once both sides are validated:

- authoritative simulation tick: Candidate A / filtered `StablePlayerAi` callback;
- presentation tick: exact played tick from the match-view object (already statically traced at `+0x250`, to be revalidated in the existing camera/match-view capture path);

then pacing can happen in the Candidate A worker's own AI callback rather than in a native runner detour. For one designated player callback per tick, the worker can wait only when:

```text
simulation_tick > played_tick + allowed_lead
```

This would slow only Candidate A's background simulation and leave the render/UI thread free to advance presentation. Manual `InputV1` would then be returned only on callbacks running on that confirmed Candidate A worker.

This design is preferable to a shared runner-code patch because it uses the official stable AI callback as the cooperative pacing/decision point and uses the native hook only to identify the correct client simulation job and read otherwise-unexposed presentation/camera state.

## Safety sequence

After the runner-loop crash, do not combine recovery and new instrumentation in one physical test.

1. First confirm the rollback build launches normally.
2. Then add the read-only Candidate-A-thread `StablePlayerAi` observer and tick diagnostics.
3. Validate simulation tick versus played tick with no sleeping and no manual input.
4. Only after that add bounded pacing.
5. Only after pacing is stable reconnect RMB/manual `InputV1` commands.
