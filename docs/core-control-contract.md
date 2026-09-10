# Direct-control core contract

The direct-control layer is intentionally a low-policy primitive rather than a player-team feature.

## Slot addressing

The base control surface exposes all ten match player slots symmetrically:

```text
F1  -> player slot 0
F2  -> player slot 1
...
F10 -> player slot 9
```

The core does **not** decide which team belongs to the human manager and does not reject a slot because of team ownership. Team/ownership restrictions, if desired, belong to a higher-level mod or feature built on top of this control layer.

This is deliberate. The project should remain useful as a dependency/foundation for future mods that may want to control either side, spectate/control arbitrary actors, build debugging tools, implement alternate game modes, or impose their own permissions.

## Primitive responsibilities

The base layer should provide mechanisms such as:

- keep the watched simulation live near wall-clock time;
- address a raw player slot;
- publish movement/action/skill inputs for that slot;
- expose cursor-to-simulation coordinates;
- expose generic entity selection data;
- irreversibly release live control and let the simulation finish.

It should avoid policy such as:

- identifying "my team" unless a higher layer explicitly asks for it;
- deciding which side may be controlled;
- enforcing game-mode-specific permissions;
- choosing strategic behavior for uncontrolled actors;
- adding feature-rich UX that constrains future consumers.

## Release behavior

`Ctrl+End` is the base one-way release primitive for the current match. Once invoked, pacing and manual control are released, the simulation may race ahead to completion, and live manual control cannot safely resume until the next match.
