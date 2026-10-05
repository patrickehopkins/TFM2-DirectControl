# Multiplayer reconnaissance plan

Status: **future research plan; no multiplayer compatibility claim.**

## Goal

Determine whether direct manual control can be made deterministic and synchronized across TFM2 multiplayer peers **before** designing any custom netcode.

Do not begin with rollback, state snapshots, reconciliation, or a user-facing multiplayer mode. First establish what TFM2 already synchronizes and where Harbinger's external human input crosses that boundary.

## Core questions

1. Do both multiplayer clients run equivalent Candidate-A/watch-match simulations?
2. Do corresponding simulation ticks match?
3. Do entity IDs, teams, positions, HP, cooldowns, inventory, and other stable state match at the same tick?
4. Is one peer/host authoritative, or are both clients running deterministic peers?
5. What native events/packets occur at match start, pause, manager interactions, and any other synchronized controls?
6. Can both peers observe the same vanilla AI InputV1 decisions?
7. If one client injects a harmless manual InputV1 and the other does not, what happens?
8. Does TFM2 replicate the input, reconcile state, detect a desync, or simply diverge?

## First milestone: observe only

Instrument both clients without enabling manual control.

Record at regular deterministic checkpoints:

- simulation tick;
- stable athlete/entity identity;
- selected representative entity state;
- controlled-team/player state where available;
- a compact deterministic state hash.

The exact hash schema should begin small and stable. Avoid hashing presentation-only state.

Example conceptual record:

```text
tick=12600
entity=player_3
pos=(...)
hp=...
cooldowns=(...)
state_hash=...
```

The purpose is to prove whether two ordinary multiplayer clients remain equivalent under vanilla play.

## Second milestone: one harmless divergent input

Only after ordinary state equivalence is demonstrated:

- choose one clearly identified champion;
- at a known tick, inject one obvious but low-risk `MoveTo(X,Y)` on **one client only**;
- do not inject the same command on the other peer;
- record both clients at high detail around that tick.

Possible outcomes:

### Best case

The other peer reflects the movement without Harbinger transmitting anything.

Implication: a native synchronized player-input/action path may already exist and multiplayer Direct Control could be much easier than expected.

### Middle case

Only the injecting peer moves and simulations diverge.

Implication: Harbinger may need its own synchronized external-input transport. Continue only after measuring how deterministic both peers are absent that external input.

### Authoritative/reconciliation case

The injecting peer is corrected, rejected, or the other side's state wins.

Implication: identify the host/server authority path before designing Harbinger networking.

### Hard failure

The game detects/desyncs/terminates or simulation identity differs unpredictably.

Implication: stop feature work and investigate determinism/authority first.

## If Harbinger must transport inputs

The first prototype should synchronize **commands**, not complete game state.

Conceptual packet:

```text
match/session id
simulation tick
controlled stable identity
command kind
command payload
sequence number
```

Example:

```text
tick=12540
slot=F3 / stable athlete identity
MoveTo(682311,417992)
```

Both peers queue the same external command for the same authoritative simulation tick.

Do not initially send render positions, camera state, UI state, or replay/presentation state.

## Determinism checks

Add periodic state hashes. If peers disagree:

- stop treating the prototype as working;
- preserve the input stream and both state traces;
- identify the first divergent tick;
- determine whether divergence came from command timing, nondeterministic game state, local-only data, or presentation/simulation confusion.

Only after the source of divergence is understood should rollback or snapshot correction even be considered.

## Input rate

Do not assume held RMB requires 60 network messages per second.

A future transport can represent changes in intent:

- start/update movement target;
- start/update exact target;
- attack-move destination;
- skill cast intent/confirmation;
- Hold;
- Return;
- selection/ownership changes.

The simulation can continue emitting the active command locally between changes if deterministic behavior is proven.

## Multiplayer invariants

Until research disproves the need:

- lock multiplayer to 1x / 60 Hz;
- disable experimental speed changes;
- do not expose replay seeking while live control is owned;
- treat presentation synchronization separately from deterministic simulation synchronization;
- use stable athlete/entity identity, never presentation card text or display names.

## Long-term architecture opportunity

If this succeeds, do not build the result as a Control-specific or Harbinger-only one-off.

The desirable reusable layer is conceptually:

> **harbinger-net: deterministic synchronization of external TFM2 simulation inputs**

A downstream mod should be able to provide its local manual command stream while Harbinger infrastructure handles:

- tick agreement;
- stable identity;
- command serialization;
- peer delivery;
- ordering/deduplication;
- state-hash diagnostics;
- desync reporting.

That would let future mods concentrate on gameplay/UI rather than reinventing multiplayer synchronization.

## Non-goals for the reconnaissance phase

- rollback;
- prediction;
- latency compensation;
- state snapshots;
- matchmaking;
- NAT traversal;
- anti-cheat;
- spectator networking;
- polished multiplayer UI.

Those are all downstream questions. First establish TFM2's native synchronization model and whether identical externally supplied InputV1 commands remain deterministic.
