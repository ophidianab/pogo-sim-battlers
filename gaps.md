# Specification Gap Checklist

This checklist reflects the accepted design baseline captured in the project specification and blueprint. These items are considered resolved and documented; remaining work is implementation detail, not unresolved design ambiguity.

## Status Summary

The main design gaps have been closed. The project now has an accepted architecture, deterministic turn semantics, a player-facing observation/action contract, compatibility targets, and a validation strategy.

## Resolved Gap Categories

### 1. Canonical runtime data contracts

- [x] Defined the central immutable game-state snapshot model.
- [x] Defined the participant roster and team composition schema.
- [x] Defined the per-Pokémon runtime state schema, including HP, energy, cooldowns, buffs, shields, and status.
- [x] Defined the normalized action schema used by the engine.
- [x] Defined the observation payload schema exposed to players.
- [x] Defined the battle result payload schema, including timeline and rating output.

### 2. Turn-resolution semantics

- [x] Documented the lifecycle of a battle turn.
- [x] Defined action collection and ordering for each participant.
- [x] Resolved fast moves, charged moves, swaps, waits, and forced swaps.
- [x] Defined end-of-turn damage and energy-gain application.
- [x] Defined forced-swap behavior when a Pokémon faints.
- [x] Defined deterministic tie-breaking and priority rules.
- [x] Defined fallback behavior for invalid or incomplete actions.

### 3. Player interface contract

- [x] Versioned the observation payload and compatibility rules.
- [x] Specified visible vs. hidden participant data.
- [x] Defined the shared action request/response contract.
- [x] Defined fallback behavior for missing or invalid decisions.
- [x] Clarified that the action contract is shared across human, script, and RL players.

### 4. State history and replayability

- [x] Resolved the replay model as snapshot-plus-event log.
- [x] Defined the event taxonomy and accepted event names.
- [x] Defined how timeline entries are emitted and ordered.
- [x] Defined serialization/export rules for debugging and replay.

### 5. Compatibility mapping to pvpoke runtime behavior

- [x] Mapped the engine’s canonical runtime state to the pvpoke compatibility baseline.
- [x] Confirmed the turn-order semantics preserved in the new engine.
- [x] Documented engine decisions that intentionally differ from the original runtime model.
- [x] Defined the 1v1 compatibility baseline and validation target.
- [x] Defined the expected timeline event names and their meanings.

### 6. Implementation boundaries and module structure

- [x] Defined the engine module layout.
- [x] Defined module ownership and responsibilities for battle state.
- [x] Defined module boundaries for player-facing interfaces.
- [x] Defined the TUI boundary as separate from engine logic.
- [x] Defined serialization/export boundaries for replay and analysis.

### 7. Validation and acceptance criteria

- [x] Defined deterministic 1v1 parity validation strategy.
- [x] Defined regression tests for turn ordering and swap timing.
- [x] Defined tests for invalid action fallback behavior.
- [x] Defined replay/export verification tests.
- [x] Defined pass/fail compatibility criteria for pvpoke-style result semantics.

### 8. Open design decisions

These are no longer unresolved specification gaps; they are implementation follow-ups or future extension topics:

- [x] Initial milestone team shape and scaling assumptions are documented.
- [x] The engine is modeled as an immutable snapshot transition system with explicit event logging.
- [x] TUI launch and verification use are documented as a debugging layer.
- [x] Serialization format is defined as snapshot-plus-event log for replay and debugging.
- [x] Unsupported future action types are handled via normalization or rejection with deterministic fallback.

## Current Status

The specification is ready to move into implementation planning. The remaining work is execution-level work, not unresolved design work.

## Recommended Next Step

Proceed with the engineering implementation plan:

- define the Rust module tree
- map the JSON-style snapshot contract into Rust structs
- implement the immutable state and transition loop
- add deterministic test fixtures for representative turn-order scenarios
- verify the 1v1 compatibility baseline against the documented output contract
