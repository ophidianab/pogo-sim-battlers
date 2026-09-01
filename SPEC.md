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
- each Pokémon records HP, energy, cooldowns, statuses, and current move availability
- battle-relevant data remains derived from the central immutable turn snapshot

## 4. Turn Resolution Model

### 4.1 State transition function

The central loop should be expressed as a pure transition:

- input: current immutable game state snapshot and participant action requests
- output: next immutable game state snapshot

This is the core rule for the battle engine. Every turn is a transition from one snapshot to another.

### 4.2 Action collection

At the start of each turn, the engine asks each registered participant for an action. The action request must be driven by the participant’s visible information view rather than the full engine state.

Actions may include:

- fast move
- charged move
- swap
- wait
- any future battle-specific action type that can be normalized into the engine’s action model

### 4.3 Action resolution order

The engine resolves actions according to the deterministic battle rules already established by the existing system, including:

- priority and turn ordering
- swap timing relative to incoming damage
- charged attack start and completion timing
- end-of-turn damage resolution and energy gain timing
- forced-swap behavior when a Pokémon faints

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

The player interface should return a normalized action decision object, not a direct mutable reference into the engine. This keeps the player side decoupled from the engine internals and makes the interface suitable for:

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

## 7. Compatibility and Migration Strategy

### 7.1 Early milestone behavior

The immediate implementation should remain compatible with the current 1v1 battle structure so that legacy logic can be validated before broadening the engine.

This includes:

- preserving the existing turn cadence and resolution semantics where applicable
- keeping the first milestone focused on a single battle shape while the architecture scales
- confirming that the new action and state interfaces still work for the basic 1v1 path

### 7.2 Scaling toward NvN

Once the 1v1 compatibility contract is stable, the engine can be generalized by:

- widening the participant roster model
- expanding the visible-state schema to support multi-participant observations
- generalizing the action resolution stage from pairwise to any number of actors
- preserving the same immutability and replay principles across all battle sizes

## 8. Required Runtime Properties

The implementation should guarantee the following properties:

- deterministic turn ordering under a fixed input set
- reproducible battle histories for audit and replay
- separation between engine runtime state and player-side decision logic
- robust fallback behavior for invalid or incomplete actions
- safe termination when turn or time budgets are exceeded
- logging/export of every state transition for later investigation

### 8.1 Battle output result contract

For the initial 1v1 compatibility milestone, the engine must expose a battle result payload whose semantics match the documented pvpoke-runtime output contract.

The result object must include:

- a `timeline` of ordered battle events, including expected event types such as `tap interaction`, `charged <type>`, `switchAvailable`, `faint`, and `tap interaction wait`
- a per-side `rating` value derived from the final remaining HP and inflicted damage, using the same formula documented in the pvpoke battle specification:

$$
\text{rating} = \left\lfloor 500 \cdot \frac{\text{opponent HP damage}}{\text{opponent max HP}} + 500 \cdot \frac{\text{self remaining HP}}{\text{self max HP}} \right\rfloor
$$

- a battle outcome summary sufficient to determine winner/loser and to compare the milestone output against the current pvpoke engine’s recorded battle results

This contract is the compatibility target for the first milestone: the simulator’s 1v1 result should be substantially similar to the existing pvpoke battle result value, so that the same encounter can be used as a verification baseline.

## 9. Success Criteria

The work is considered complete when:

- the engine can represent a battle as a sequence of immutable turn snapshots
- the simulation loop is driven by a central global state object
- player entities are registered externally and act only through a limited observation/action interface
- the battle history is exportable for review and analysis
- the TUI exists as a separate component for manual verification
- the architecture supports 1v1 milestone usage while remaining extensible to generic NvN battles
