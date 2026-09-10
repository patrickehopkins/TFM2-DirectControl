# Direct-control core contract

The direct-control layer is intentionally a low-policy primitive rather than a player-team feature.

## Actor addressing

Teamfight Manager 2 exposes ten visible match cards labeled F1-F10, but runtime testing proved that this visible order is **not** the same as Candidate A's internal `player_id` ordering. The core therefore must not define `F3 == player_id 2` or similar arithmetic mappings.

For the current UI convenience layer:

```text
F1-F10 visible card
        -> displayed athlete identity
        -> stable athlete id
        -> Candidate-A StableAiContext::athlete_id()
```

The core does **not** decide which team belongs to the human manager and does not reject an athlete because of team ownership. Team/ownership restrictions, if desired, belong to a higher-level mod or feature built on top of this control layer.

This is deliberate. The project should remain useful as a dependency/foundation for future mods that may want to control either side, spectate/control arbitrary actors, build debugging tools, implement alternate game modes, or impose their own permissions.

## Primitive responsibilities

The base layer should provide mechanisms such as:

- keep the watched simulation live near wall-clock time;
- identify/address a live actor without assuming team ownership or UI-order == internal-id order;
- publish movement/action/skill inputs for that actor;
- expose cursor-to-simulation coordinates;
- expose generic entity selection data;
- explicitly start the live simulation when startup gating is enabled;
- irreversibly release live control and let the simulation finish.

It should avoid policy such as:

- identifying "my team" unless a higher layer explicitly asks for it;
- deciding which side may be controlled;
- enforcing game-mode-specific permissions;
- choosing strategic behavior for uncontrolled actors;
- adding feature-rich UX that constrains future consumers.

## Start/release behavior

Stage 5A uses `Ctrl+Home` as an explicit start gate while testing whether the client can finish loading the battlefield with Candidate A held at its first simulation callback.

`Ctrl+End` is the base one-way release primitive for the current match. Once invoked, pacing and manual control are released, the simulation may race ahead to completion, and live manual control cannot safely resume until the next match.
