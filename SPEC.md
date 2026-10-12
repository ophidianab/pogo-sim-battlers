# Project Specification

This file describes the overall design and implementation plan for the Pokémon GO battle simulation engine and its supporting tooling.

## 1. Goal

The project’s long-term target is a fully generic NvN battle simulator, with the current milestone still preserving the existing 1v1 battle behavior as the initial compatibility baseline. In practical terms, the standard battle structure for early production use is expected to be 3v3, but the runtime architecture must be capable of scaling beyond that shape without redesign.

The engine should therefore model battles as deterministic, turn-based state machines that can be executed for any number of participants while maintaining strong compatibility with the existing battle logic used in the pvpoke repository.

## 2. Architectural Principles

### 2.1 Centralized state ownership

After initialization, all simulation-relevant data must live in a single central game-state structure. That structure is the source of truth for the current turn and is passed through the turn loop as an immutable snapshot.

The battle loop must never mutate shared state in place during decision-making. Instead, each turn resolves a set of actions against the current immutable state and produces a new future state for the next turn.

### 2.2 Immutable turn snapshots

Each turn is represented as an immutable snapshot of game state. The snapshot contains all information needed to resolve the next action phase:

- participant roster and team composition
- active slot assignments
- current HP, energy, cooldowns, and status effects
- pending effects and queued actions
- turn index and event metadata

The chain of snapshots produced over the course of a battle should be exportable and loggable for analysis, replay, and debugging.

### 2.3 Player entities are registered, not stateful

Player entities are initialized into the simulation as participants, but they are not allowed to own or mutate battle state that the simulation loop depends on. A player’s role is only to answer an action request when the engine asks for one.

This means:

- player state is external to the battle state machine
- player logic is stateless from the engine’s perspective
- the simulation does not rely on hidden player-side mutation
- the same player registration can be reused across many simulations

### 2.4 Restricted visibility for decision-making

A player entity must not receive access to the full game state. Instead, it should be given a limited, explicit “visible information” view that contains only the data needed to make a valid decision.

The visibility layer should be designed so that:

- the engine can expose a structured observation object
- a human TUI can consume the same observation model for display
- a reinforcement learning harness can use the same interface for action selection

This makes the player interface suitable for training and evaluating agents under a consistent observation/action contract.

## 3. Simulation Model

### 3.1 Engine

The engine lives in the `engine/` directory and is responsible for:

- managing battle initialization
- materializing a global game state snapshot
- collecting participant actions for a turn
- resolving action ordering and timing
- producing the next turn’s immutable game state
- emitting a replayable history of states and events

### 3.2 Battle structure

The engine must support a generic participant count, but milestone implementations should still intentionally target the existing 1v1 case and the standard 3v3 battle model.

Core invariants:

- battles are discrete-turn, deterministic simulations
- each turn advances one unit of battle progression
- one or more participants may act per turn according to the engine rules
- the engine supports both symmetrical and asymmetrical team compositions
- all battle decisions are resolved through the global state transition function

### 3.3 Participant model

A participant represents a player or controller side within the simulation. A participant may own multiple Pokémon, but the simulator must not require the participant type itself to hold mutable battle state.

The participant model should support:

- registration during initialization
- action selection via a limited observation interface
- deterministic, reproducible action requests
- optional human or automated decision backends

### 3.4 Team and Pokémon model

The simulator should continue to use a team-based structure for Pokémon GO-style combat:

- each participant may have one active Pokémon and a bench or reserve set
- a team may be 1-to-3 Pokémon in the current 1v1 compatibility path, and can be generalized to a larger roster for NvN
- battle setup receives each Pokémon's species, level, IV spread, and shadow status, then derives its initialized HP and combat stats
- each Pokémon records HP, energy, cooldowns, statuses, and current move availability
- each Pokémon's battle state includes initialized effective attack and defense values derived from its species, IVs, and level/CP multiplier; these are stored before in-battle stat stages and shadow modifiers
- the level-to-CPM values are a data asset indexed directly at half-level increments; unsupported levels are rejected rather than interpolated
- battle-relevant data remains derived from the central immutable turn snapshot

## 4. Turn Resolution Model

### 4.1 State transition function

The central loop should be expressed as a pure transition:

- input: current immutable game state snapshot and participant action requests
- output: next immutable game state snapshot

This is the core rule for the battle engine. Every turn is a transition from one snapshot to another.

### 4.2 Action collection

At the start of each turn, the engine collects **move action requests** from each registered participant, driven by the participant's visible information view:

- fast move
- charged move
- swap
- wait
- any future battle-specific action type that can be normalized into the engine's action model

During turn resolution, when a charged move is queued to hit a target, the engine requests a **shield decision** from the defending participant. This decision is made within the turn, does not consume a turn or action, and is specific to that incoming charged move. The defender makes the shield decision based on their observation state—they know a charged move is incoming but do not see the specific move identity or base damage.

### 4.3 Action resolution order

The engine resolves actions according to the deterministic battle rules already established by the existing system, with the following specifics:

#### Turn progression and fast-move duration

- Fast moves have a duration of one or more turns. An in-progress fast move from a prior turn may "lock" a participant, preventing other actions until the duration completes.
- If both participants have fast moves in-progress on the same turn, only the turn clock advances (durations decrement); no other actions are processed.
- When a participant uses a charged move, any opponent in-progress fast move completes immediately as part of that turn's resolution.

#### Action execution priority

Within a turn, actions execute in this order:

1. **Swaps** (all swaps execute first, before any damage is applied)
2. **Charged moves** (ordered by CMP: Charged Move Priority)
3. **Fast moves** (damage queued for end-of-turn application)

#### Charged Move Priority (CMP)

When multiple participants use charged moves on the same turn:
- The participant with the higher attack stat has their charged move resolve first
- If attack stats are equal, left-to-right participant ordering breaks the tie
- **Cascade consequence**: if a charged move resolves first and causes the target to faint, the target's queued charged move does not fire, and any in-progress fast move is cancelled

#### Charged move resolution with shield decision

When a charged move is being applied to a target:

1. Request shield decision from the target (asynchronous, within the turn, does not advance turn counter)
2. Calculate base damage
3. If defender chose to shield, reduce damage to 1 and consume one shield
4. Apply final damage to defender
5. Apply stat changes and buffs/debuffs
6. Check for faint and cascade

#### End-of-turn resolution

After all charged and fast move actions are queued:
- Apply all queued fast move damage
- Apply all energy gains (energy gained in turn N is available starting turn N+1, not usable in the same turn)
- Reset fast-move cooldowns for completed moves
- Detect faints from queued damage
- Process forced swaps (auto-select next available bench Pokémon if active Pokémon faints)
- Record timeline events

#### Determinism guarantees

These rules ensure the following:

- **Simultaneous KOs are possible**: both participants can faint on the same turn if both survive each other's charged moves and their fast-move damage both apply
- **No energy can be spent in the turn it is gained**: energy becomes available only in the following turn
- **Swaps resolve before damage**: swap timing is deterministic and cannot be affected by incoming damage
- **CMP controls charged move ordering**: higher attack stat moves resolve first; if fatal, the opponent's charged move is cancelled
- **Forced-swap behavior is deterministic**: faints trigger automatic swap to the next available bench Pokémon at the end of the turn

These rules remain the basis for correctness, even as the number of participants grows from 2 to N.

### 4.4 State history and replayability

The engine should retain the full sequence of turn states produced during a battle.

This history must support:

- battle replay
- debugging of incorrect or surprising outcomes
- review of turn-by-turn decisions
- export to external analysis tools

A compliant implementation should expose a replay log or serialized state trace that can be examined after simulation completion.

## 5. Player Interface Contract

### 5.1 Observation contract

The player interface must not reveal the full internal engine state. Instead it should expose a stable, versioned observation payload containing only visible, game-meaningful data such as:

- visible team composition and active Pokémon
- current HP and energy values that are observable in-game
- active status effects and cooldown windows that are visible to the participant
- known move availability and charging state
- the current turn number or turn phase
- a limited view of opponent information permitted by the ruleset

### 5.2 Action contract

The player interface should return a normalized action decision object per turn, not a direct mutable reference into the engine state. This keeps the player side decoupled from the engine internals.

Move action families:

- fast move
- charged move
- swap
- wait
- future normalized battle actions

**Shield decisions are separate**: When a charged move targets the defender, the engine requests a binary shield/no-shield decision during turn resolution. This decision is asynchronous, does not advance the turn counter, and is based on the defender's observation state. The defender does not see the incoming move identity or base damage.

This contract makes the interface suitable for:

- local human play
- scripted agents
- reinforcement learning models
- future search or minimax evaluators

### 5.3 RL suitability

The player interface should be structured to support a reinforcement-learning harness by ensuring:

- observations are deterministic and serializable
- actions are encoded into a fixed schema
- no hidden engine state leaks into the observation channel
- the same battle snapshot can be re-fed into an agent repeatedly for training and evaluation

## 6. TUI Component

A simple Text User Interface (TUI) must exist for manual testing of the simulator. The TUI is a separate component from the engine and should provide:

- a basic terminal-based view of the current game state
- a way for a human player to submit actions during a battle
- a clear display of turn progression, HP, energy, and important battle events
- an obvious separation between engine logic and presentation concerns

The TUI should be intentionally lightweight. It exists for debugging and interactive validation, not as a replacement for the engine or a general-purpose game client.

## 7. Accepted compatibility and ownership decisions

The design baseline has been clarified and accepted for implementation as follows:

### 7.1 Canonical runtime mapping

- The canonical battle state remains the engine’s source of truth for HP, energy, initialized combat stats, shields, team state, and turn progression.
- pvpoke runtime semantics are used as compatibility tests and validation targets, not as a direct internal schema mirror.
- The engine must preserve timing semantics even when the internal state representation differs from the reference runtime.

### 7.2 Timing semantics

- Damage resolves at the end of each turn.
- Charged Attacks begin at the start of the following turn.
- Energy gained during a turn is not spendable until the next turn.
- A valid charged move remains able to resolve in the attack sequence even when the target would otherwise faint from a fast move.

### 7.3 Shield and damage mitigation

- Shield decisions are requested during turn resolution and do not advance the turn counter.
- Shields mitigate pending incoming damage before final HP reduction.
- If a shield decision is missing or malformed, the deterministic fallback is no-shield.

### 7.4 Swap semantics

- Swaps resolve before damage.
- Quick swaps cost a full turn.
- Forced swaps after fainting do not consume a turn.
- Swap execution remains deterministic and independent of input timing variance.

### 7.5 Module ownership and boundaries

- The engine owns the immutable battle snapshot and the turn transition function.
- The player layer owns observation generation and normalized action selection.
- The resolver owns action ordering, shield resolution, and combat effects.
- Replay/history and presentation code are external to live battle state and must not mutate engine state in place.

### 7.6 Validation strategy

Validation must be treated as part of the engine contract and must include:

- deterministic turn-ordering tests
- snapshot-transition output assertions
- replay fidelity and event-log validation
- observation compatibility and visibility checks
- battle regression cases for shield, swap, and charged-move timing edge scenarios

## 8. Compatibility and Migration Strategy

### 8.1 Early milestone behavior

The immediate implementation should remain compatible with the current 1v1 battle structure so that legacy logic can be validated before broadening the engine.

This includes:

- preserving the existing turn cadence and resolution semantics where applicable
- keeping the first milestone focused on a single battle shape while the architecture scales
- confirming that the new action and state interfaces still work for the basic 1v1 path

### 8.2 Scaling toward NvN

Once the 1v1 compatibility contract is stable, the engine can be generalized by:

- widening the participant roster model
- expanding the visible-state schema to support multi-participant observations
- generalizing the action resolution stage from pairwise to any number of actors
- preserving the same immutability and replay principles across all battle sizes

## 9. Required Runtime Properties

The implementation should guarantee the following properties:

- deterministic turn ordering under a fixed input set
- reproducible battle histories for audit and replay
- separation between engine runtime state and player-side decision logic
- robust fallback behavior for invalid or incomplete actions
- safe termination when turn or time budgets are exceeded
- logging/export of every state transition for later investigation

### 9.1 Battle output result contract

For the initial 1v1 compatibility milestone, the engine must expose a battle result payload whose semantics match the documented pvpoke-runtime output contract.

The result object must include:

- a `timeline` of ordered battle events, including expected event types such as `tap interaction`, `charged <type>`, `switchAvailable`, `faint`, and `tap interaction wait`
- a per-side `rating` value derived from the final remaining HP and inflicted damage, using the same formula documented in the pvpoke battle specification:

$$
\text{rating} = \left\lfloor 500 \cdot \frac{\text{opponent HP damage}}{\text{opponent max HP}} + 500 \cdot \frac{\text{self remaining HP}}{\text{self max HP}} \right\rfloor
$$

- a battle outcome summary sufficient to determine winner/loser and to compare the milestone output against the current pvpoke engine’s recorded battle results

This contract is the compatibility target for the first milestone: the simulator’s 1v1 result should be substantially similar to the existing pvpoke battle result value, so that the same encounter can be used as a verification baseline.

## 10. Success Criteria

The work is considered complete when:

- the engine can represent a battle as a sequence of immutable turn snapshots
- the simulation loop is driven by a central global state object
- player entities are registered externally and act only through a limited observation/action interface
- the battle history is exportable for review and analysis
- the TUI exists as a separate component for manual verification
- the architecture supports 1v1 milestone usage while remaining extensible to generic NvN battles
