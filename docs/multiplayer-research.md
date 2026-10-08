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

## October 8, 2026: cost constraint and external references

**Status: research only. No networking implementation or dependency has been selected; Harbinger remains single-player.**

### Non-negotiable operating budget

The finished multiplayer feature must not require **any recurring developer/operator expenditure** for hosting, relays, sessions, matchmaking, domain renewal, dedicated servers, usage-metered services, or network access beyond participants' ordinary internet connections. The infrastructure budget is **$0.00/month**. A reasonably priced **one-time license** may be reviewed separately; purchasing it is not authorized by this document.

Prefer player-hosted matches and an optional direct-connection path. A provider advertised as free can be investigated, but do not assume a permanent entitlement, unrestricted traffic, relay capacity, or permission for use from a third-party game mod. Record service terms, rate limits, authentication requirements, game/app ownership requirements, potential quota exhaustion, and what happens if the service disappears. Never make a mandatory paid fallback part of the design.

**Network reachability is a general problem:** consumer routers, firewall policy, carrier-grade NAT, NAT traversal failures, and non-forwardable connections need eventual testing. Do not infer a particular personal ISP or network topology. Do not require a static public IP or router configuration from ordinary players.

### Separate the two gates: simulation vs. transport

**Gate A — Native simulation/authority:** prove that an input from outside the local game's usual keyboard/controller path can legally affect a *running authoritative match* at an agreed tick, rather than only the viewed replay or a local-only simulation. The existence of \`StablePlayerAi::think()\` / \`InputV1\` is not proof of safe live network integration. Identify the native data boundary and deterministic rules; do not read sockets, OS clocks, mutable external queues, or nondeterministic state directly inside deterministic simulation callbacks.

**Gate B — Player connectivity:** only after Gate A is demonstrated, choose a transport for ordered/tick-addressed inputs, acknowledgments where necessary, peer/host roles, session binding, reconnect policy, and state hashes. A successful LAN socket test proves nothing about Gate A, and a successful single-player manual-input mod proves nothing about Gate B.

If Gate A fails through the stable API, distinguish *stable API limitation* from *underlying engine impossibility*. Investigate, in order: an upstream SDK request for synchronized external inputs, documented native multiplayer command ingress, carefully version-checked native hooks, and a separately tested host-side command interface. A lower-level approach must fail closed when game signatures change. Do not claim that a WebSocket, relay, VPN, or library can bypass an engine that cannot accept external commands. If no safe authoritative-tick ingress can be demonstrated, stop multiplayer implementation and document that blocker.

### Updated staged experiments

0. **Single-machine input-boundary proof:** in a local test match, feed a synthetic, tick-labelled harmless command via a non-keyboard producer and check which simulation (live authoritative vs. replay presentation) consumes it. This is a controlled feasibility probe, not multiplayer support.
1. **Unmodified vanilla two-player baseline:** compare two instances' tick numbers, stable identities and canonical state hashes; learn which instance(s) simulate and who has authority.
2. **One-sided controlled divergence:** perform the previously specified harmless \`InputV1\` experiment at a recorded tick. Observe whether the game replicates, ignores, corrects or diverges.
3. **Deterministic ingress prototype:** if feasible, serialize one remote command with session ID, tick, stable champion identity, kind, payload and sequence; apply it only at the validated ingress boundary. Measure latency and replay/simulation drift without assuming rollback.
4. **Free two-machine transport:** begin with LAN or direct networking. Compare optional free relay/NAT traversal services only when needed. Test disconnects, duplicates, out-of-order delivery, packet loss, and inconsistent inputs; measure state hashes.
5. **Capacity research:** start from the game's ordinary two-player session. Only after verified ownership, session and authority behavior should an aspirational **up-to-ten independent controllers** be investigated. This is **not** a current supported feature.

Do not create a standalone network laboratory merely to recreate capabilities already offered by GGPO sync tests, existing impairment tools, or the game's own instrumentation.

### Reference hierarchy (not technology commitments)

| Reference | Research value | Adoption status |
| --- | --- | --- |
| [Team Samoyed stable mod SDK](https://github.com/TeamSamoyed/TeamfightManager2Mod) | Native deterministic input and multiplayer API rules; first authority for this mod | API to inspect/validate against installed game version |
| [Gaffer On Games — Networked Physics](https://gafferongames.com/categories/networked-physics/) | Lockstep versus state synchronization, host authority, packet/timing constraints | Design reading only |
| [Valve — Source Multiplayer Networking](https://developer.valvesoftware.com/wiki/Source_Multiplayer_Networking) | Commands, server authority, interpolation, prediction | Design reading only |
| [GGPO Developer Guide](https://github.com/pond3r/ggpo/blob/master/doc/DeveloperGuide.md) | Determinism tests, input logs, state serialization and *rollback prerequisites* | Comparative reference; **do not assume rollback is possible in TFM2** |
| [GGRS](https://github.com/gschup/ggrs) | Alternate input-sync and sync-test examples | Comparative reference only |
| [Drift Engine](https://driftengine.dev/) | Compact illustrative architecture for rewinds, input history and test visualization | **Small/less-established example only; not a selected engine, transport or dependency** |

**Connectivity candidates (all unselected):** direct UDP or WebRTC (no required operator fee but NAT limitations); Epic Online Services networking/relay (check [license](https://onlineservices.epicgames.com/licensing), authentication, third-party-mod eligibility and availability); Steam networking (do **not** assume our mod may use TFM2's Steam application networking identity); temporary personal overlays such as Tailscale/ZeroTier for a small test cohort (free-tier device/user limits); and WebSocket command bridging through Cloudflare Tunnel only where its applicable terms and traffic requirements fit. **Cloudflare Spectrum is paid and not a $0 general UDP game-hosting solution.** A free tunnel does not imply permanent free domain registration or production-grade game-transport guarantees.

The native simulation feasibility result, not the choice of networking provider, determines whether TFM2 multiplayer is possible. Do not introduce Drift as a presumed solution, and do not rewrite this research plan as a claim of shipping multiplayer.
