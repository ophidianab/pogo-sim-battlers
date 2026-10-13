# Engineering Blueprint Draft

Status: Draft derived from the project specification and supporting design notes.

## 1. Objective

Create a deterministic, turn-based Pokémon GO battle engine that preserves a 1v1 compatibility baseline while remaining extensible toward generic NvN battle execution.

The implementation should be structured around:

- one central immutable game-state snapshot per turn
- external participant registration with limited observation/action contracts
- a pure state transition model
- replayable battle history and timeline export
- a lightweight TUI for manual verification

## 2. Product Constraints

### Core engine constraints

- The engine must be deterministic under a fixed input set and random seed.
- Battle logic must operate from immutable snapshot transitions.
- Player logic must remain external to the engine runtime state.
- The engine must support a 1v1 milestone while remaining scalable to larger participant counts.

### Compatibility constraints

- The initial result contract should preserve pvpoke-style result semantics.
- The battle result payload must include a timeline, outcome summary, and per-side rating.
- The timeline should include event types such as `tap interaction`, `charged <type>`, `switchAvailable`, `faint`, and `tap interaction wait`.

## 3. Architectural Overview

### 3.1 Runtime shape

The runtime should be organized as a layered system:

1. Data layer
   - battle data lookup
   - move and Pokémon definitions
   - game metadata and encounter fixtures

2. Engine layer
   - battle initialization
   - immutable state management
   - action collection
   - action ordering and resolution
   - next-state transition generation
   - history/timeline export

3. Participant layer
   - registered external decision makers
   - observation generation
   - normalized action return

4. Presentation layer
   - lightweight TUI for debugging and manual validation
   - separate from engine correctness logic

## 4. State Model

### 4.1 Central state object

All battle-relevant state should be stored in a single central snapshot object that is treated as the source of truth for the current turn.

Contract style decision:

- The immutable snapshot contract will be defined first as a stable JSON-style schema.
- The Rust implementation will map that schema into concrete structs during implementation.
- The schema is the external contract; the Rust model is the internal representation.

Required categories of data:

- participant roster
- team composition and active slots
- current HP and energy
- cooldown information
- move-defined effects and current stat stages
- queued actions and pending effects
- turn index and metadata

Initial JSON-style shape for the snapshot root:

```json
{
  "battleId": "string",
  "turn": 0,
  "participants": [],
  "timeline": [],
  "metadata": {}
}
```

The `participants` array will contain ordered participant entries with per-side roster and active-slot information, and the implementation will later map this contract into Rust structs.

Battle initialization accepts a build input for each Pokémon. The caller supplies species, level, IVs, and shadow status; the initializer derives HP and effective combat stats once and stores the resulting values in the battle snapshot.

```json
{
  "speciesId": "bulbasaur",
  "level": 20,
  "ivs": {
    "attack": 10,
    "defense": 10,
    "hp": 10
  },
  "shadow": false
}
```

Proposed JSON schema for the participant roster and team composition contract:

```json
{
  "participants": [
    {
      "participantId": "string",
      "controllerType": "human | ai | scripted | rl",
      "side": "left | right",
      "activePokemonIndex": 0,
      "team": [
        {
          "slot": 0,
          "pokemonId": "string",
          "species": "string",
          "isActive": true,
          "currentHp": 100,
          "maxHp": 100,
          "energy": 0,
          "statStages": {
            "attack": 0,
            "defense": 0
          },
          "cooldowns": {
            "fastMoveMs": 0,
            "chargedMoveMs": 0
          },
          "availableMoves": {
            "fast": "string",
            "charged": ["string"]
          }
        }
      ]
    }
  ]
}
```

This schema is the approved contract for the first gap category and will be mapped into Rust structs during implementation.

Proposed JSON schema for the per-Pokémon runtime state contract:

```json
{
  "slot": 0,
  "pokemonId": "string",
  "species": "string",
  "isActive": true,
  "currentHp": 100,
  "maxHp": 100,
  "combatStats": {
    "attack": 123.45,
    "defense": 145.67
  },
  "statStages": {
    "attack": 0,
    "defense": 0
  },
  "energy": 0,
  "energyGain": 0,
  "cooldowns": {
    "fastMoveMs": 0,
    "chargedMoveMs": 0,
    "swapMs": 0
  },
  "moveState": {
    "fastMove": "string",
    "chargedMoves": ["string"],
    "chargedReady": false,
    "pendingCharge": false
  },
  "flags": {
    "fainted": false,
    "shielded": false,
    "shadow": false
  }
}
```

This runtime-state schema is the accepted contract for the first gap category and is mapped to Rust structs.

`combatStats.attack` and `combatStats.defense` are the initialized battle values derived from species base stats, IVs, and the selected level/CP multiplier. They are stored in the snapshot so damage and CMP use the same deterministic values on every turn. These values are before in-battle stat-stage changes and shadow modifiers; those are applied separately during resolution.

`statStages` stores the current attack and defense stages. Each stage is an integer clamped to `-4..=4`, starts at zero, and uses a buff divisor of `4`: positive stages multiply by `(4 + stage) / 4`, while negative stages multiply by `4 / (4 - stage)`. Shadow modifiers apply separately for damage; neither stage nor shadow modifiers affect CMP, which compares initialized attack values.

Move definitions carry optional `buffs` as `[attackDelta, defenseDelta]`, `buffTarget` (`self`, `opponent`, or `both`), and `buffApplyChance` (a probability serialized as a string in the source data). These fields are present together or not at all. For example, Icy Wind uses `[-1, 0]`, targets the opponent, and has chance `"1"`. When a move effect succeeds, the applicable Pokémon's `statStages` are updated. Stage state resets to zero on switch-out; no separate status-effect list is modeled.

The CPM values are stored in `engine/data/cpm.json` in their original order, beginning at level 1.0 with a 0.5-level step. Lookup uses the exact table entry at index `(level - 1.0) / 0.5`; valid levels must fall on a listed half-level. The engine uses direct lookup only and does not interpolate or apply the legacy neighboring-entry derivation.

Initialization uses the CPM and the catalog's base stats with the supplied IVs:

- `attack = CPM * (baseAttack + attackIV)`
- `defense = CPM * (baseDefense + defenseIV)`
- `maxHP = max(floor(CPM * (baseHP + hpIV)), 10)`, except Shedinja has 10 HP
- `CP = max(floor((baseAttack + attackIV) * sqrt(baseDefense + defenseIV) * sqrt(baseHP + hpIV) * CPM^2 / 10), 10)`

The stored attack and defense are unmodified initialization values. Shadow modifiers and in-battle stat stages are applied by the battle resolver, not during this calculation. Invalid IVs and levels without an exact CPM entry are rejected.

### Type effectiveness compatibility contract

- `engine/data/type_chart.json` mirrors the defensive-type trait lists from PvPoke `DamageCalculator.js`: weaknesses, resistances, and immunities for all 18 types.
- Type names are parsed into the `PokemonType` enum; incomplete charts and unknown type names are rejected during catalog loading.
- The Pokémon data's `none` secondary-type sentinel is represented explicitly and contributes neutral effectiveness; it is not a nineteenth chart entry.
- Effectiveness starts at `1.0` and multiplies one applicable factor for each defending type, checking weakness, resistance, then immunity in that order.
- PvPoke's `immunities` use `DOUBLE_RESISTED = 0.390625`; they do not make effectiveness zero.
- The chart's list ordering is preserved in the asset. Trait-list order does not alter the result because each list is membership-tested.
- Damage constants are defined once in `DamageConstants` using the exact PvPoke values.

Proposed JSON schema for the normalized action contract:

```json
{
  "participantId": "string",
  "turn": 0,
  "actionType": "fastMove | chargedMove | swap | wait",
  "payload": {
    "fastMove": {
      "moveId": "string"
    },
    "chargedMove": {
      "moveId": "string",
      "shield": false
    },
    "swap": {
      "targetSlot": 1
    },
    "wait": {}
  }
}
```

This action schema is the accepted contract for the next clarification item and normalizes all player decisions into a single canonical form.

Proposed JSON schema for the observation payload contract:

```json
{
  "observationVersion": "2.0",
  "turn": 0,
  "self": {
    "participantId": "string",
    "side": "left | right",
    "active": {
      "species": "string",
      "currentHp": 100,
      "maxHp": 100,
      "energy": 0,
      "statStages": {
        "attack": 0,
        "defense": 0
      }
    },
    "bench": [
      {
        "slot": 1,
        "species": "string",
        "currentHp": 50,
        "maxHp": 100,
        "statStages": {
          "attack": 0,
          "defense": 0
        }
      }
    ],
    "shields": 2
  },
  "opponent": {
    "participantId": "string",
    "side": "right | left",
    "active": {
      "species": "string",
      "currentHp": 80,
      "maxHp": 100,
      "energy": 0
    },
    "benchCount": 2,
    "shields": 1
  }
}
```

This observation schema is the accepted contract and provides a restricted view suitable for human TUI rendering, scripted agents, and RL harnesses without exposing full engine state.

**Observation versioning contract:**

- `observationVersion`: semantic version (e.g., "1.0", "1.1", "2.0")
  - Major version bump: breaking schema changes (field removed or type changed)
  - Minor version bump: additive changes only (new optional fields)
  - Agents and TUI should validate and reject incompatible versions

- `schemaRevision`: monotonic integer for internal tracking and compatibility debugging

All 1.x versions maintain backward compatibility (additive only). Breaking changes move to 2.0 or later.

**Player interface compatibility policy:**
- `observationVersion`: semantic version (e.g., "1.0", "1.1", "2.0")
  - Major bump: breaking changes
  - Minor bump: additive changes only
  - Patch bump: documentation or internal-only changes
- `schemaRevision`: monotonic integer for compatibility logging and debugging
- Agents and TUI must reject incompatible major versions
- Minor versions remain compatible if the required fields remain present
- Battle result metadata should include the observation schema version for replay and debugging

**Shared player-facing contract:**
- The observation payload is the shared contract for both human and automated players.
- The TUI may enrich the same observation with presentation-only fields, but it must not mutate engine state.
- The RL harness uses the same deterministic observation payload directly for action selection.
- The action contract is shared across all player types: move actions at turn start, shield decisions only when a charged move is incoming.
- The engine is responsible for validation and fallback behavior; players never receive internal engine state.

**Visibility model for observations:**

SELF SIDE (fully visible):
- active Pokémon: species, currentHp (raw), maxHp (raw), hpPercentage, energy, statStages
- bench: slot, species, currentHp (raw), maxHp (raw), hpPercentage
- shields remaining count
- all move identities (fast and charged)
- move usage history
- switchCooldown: turns remaining before voluntary swap is available (0 = can swap now)

OPPONENT SIDE (limited visibility):
- active Pokémon: species, hpPercentage only (no raw HP values)
- bench count only (not individual Pokémon species)
- shields remaining count
- lastUsedMove: the move that resolved in the previous turn (fast or charged)
- Hidden: opponent energy, opponent move identities (except lastUsedMove), opponent cooldowns (including switchCooldown), opponent in-progress fast move state, opponent stat stages

This visibility model mirrors in-game information: players see opponent species and shield count, learn opponent moves after they resolve, track their own cooldowns for informed decisions, and use move history to predict future actions.

Proposed JSON schema for the battle result payload contract:

```json
{
  "battleId": "string",
  "outcome": "participant_0_win | participant_1_win | draw",
  "winner": "string | null",
  "loser": "string | null",
  "participants": [
    {
      "participantId": "string",
      "side": "left | right",
      "rating": 750,
      "finalHp": 42,
      "maxHp": 100,
      "opponentDamage": 58,
      "opponentMaxHp": 100
    }
  ],
  "timeline": [
    {
      "turn": 0,
      "timestamp": 0,
      "events": [
        {
          "type": "tap interaction",
          "participantId": "string",
          "moveId": "string",
          "damage": 5
        },
        {
          "type": "charged <type>",
          "participantId": "string",
          "moveId": "string",
          "damage": 35,
          "shielded": false
        },
        {
          "type": "switchAvailable",
          "participantId": "string"
        },
        {
          "type": "faint",
          "participantId": "string",
          "species": "string"
        },
        {
          "type": "tap interaction wait",
          "participantId": "string",
          "duration": 500
        }
      ]
    }
  ],
  "totalTurns": 15,
  "battleDuration": 7500
}
```

This result payload schema is the accepted contract and is the final output of the engine, providing the complete state and timeline necessary for replay, ranking, and analysis. It directly supports 1v1 parity with the pvpoke-style result contract.

**First gap category complete:** All canonical runtime data contracts have been defined and approved.

### 4.2 Immutability rule

The engine must never mutate shared battle state during decision-making.

Instead:

- current turn input is an immutable snapshot
- participant decisions are collected from that snapshot
- the engine resolves actions against the snapshot
- the result is a new immutable snapshot for the next turn

## 5. Participant and Player Model

### 5.1 Participant

A participant represents a controller side within the simulation.

It should support:

- registration during initialization
- action selection through a limited observation interface
- deterministic, reproducible action requests
- optional human or automated backends

### 5.2 Player-side restriction

The participant must not receive direct mutable access to the engine state.

It should only receive:

- limited visible team information
- visible HP/energy information
- own stat stages and visible cooldown windows
- known move availability and charging state
- turn number or phase
- explicitly permitted opponent information

## 6. Observation and Action Contracts

### 6.1 Observation contract

The observation contract should expose a stable, versioned view of the battle that is suitable for:

- human TUI rendering
- scripted or deterministic agents
- reinforcement-learning harnesses

### 6.2 Action contract

The action contract should return a normalized action decision object rather than a reference into the engine state.

Expected action families:

- fast move
- charged move
- swap
- wait
- future normalized battle actions

**Note**: Shield decisions are **not** part of the action contract. Shield decisions are requested asynchronously during turn resolution when a charged move is about to hit the target. They do not advance the turn counter and are a separate decision interface.

### 6.3 Shield decision contract

When a charged move targets a defender during turn resolution, the engine requests a binary shield decision from the defender:

- **No prior knowledge**: the defender does not see which move is incoming or its base damage
- **Based on observation**: the defender decides using their visible state (own HP, energy, opponent's visible state, remaining shields)
- **Asynchronous within turn**: the decision occurs during turn resolution and does not consume a turn or action slot
- **Deterministic fallback**: if the defender cannot or does not respond, the default is no-shield

### 6.4 Robustness requirement

Invalid or unsupported actions should degrade predictably via fallback behavior rather than causing engine corruption.

**Fallback strategy**:
- Missing or invalid move action: default to `wait`
- Invalid charged move with insufficient energy: degrade to `fast move` if legal, otherwise `wait`
- Invalid swap with no available bench: default to `wait`
- Missing or invalid shield decision: default to `no-shield`
- Timeout handling: move actions and shield decisions should have explicit timeouts and log the timeout for debugging

Shield decisions that timeout or are unavailable default to no-shield.

## 7. Turn Resolution Model

### 7.1 State transition function

The engine transition should be modeled as:

- Input: current immutable snapshot + participant action requests
- Output: next immutable snapshot

### 7.2 Action collection phase

At the start of each turn:

1. the engine asks each registered participant for an action using the observation view
2. the actions are normalized into engine action types
3. the engine validates them against current rule constraints

### 7.3 Resolution phase

The engine resolves actions according to the battle rules already documented in the spec and supporting pvpoke battle notes.

This includes:

- action priority and ordering
- swap timing relative to incoming damage
- charged attack start and completion timing
- end-of-turn damage and energy gain timing
- forced swaps after fainting

### 7.4 History generation

Every transition should emit or retain enough data to support:

- battle replay
- debugging
- turn-by-turn analysis
- export to external tools

## 8. Battle Result Contract

For the 1v1 milestone, the engine must return a battle result payload with:

- ordered timeline of events
- per-side rating
- winner/loser summary
- enough state to compare against the documented pvpoke output behavior

The rating calculation should follow the documented formula:

$$
\text{rating} = \left\lfloor 500 \cdot \frac{\text{opponent HP damage}}{\text{opponent max HP}} + 500 \cdot \frac{\text{self remaining HP}}{\text{self max HP}} \right\rfloor
$$

## 9. TUI Component

The TUI is a separate presentation layer intended for manual debugging.

Its responsibilities:

- render current game state in terminal form
- allow human input for actions during a battle
- interpret turn progression and important events
- display HP, energy, and timeline progress

The TUI must remain a thin presentation concern and must not own battle logic.

## 10. Engineering Execution Plan

### Phase 1: 1v1 compatibility baseline

- establish immutable state snapshot model
- implement minimal battle loop
- wire player action interface
- produce timeline and rating output
- validate parity against known baseline behavior

### Phase 2: generic structural expansion

- widen participant roster support
- generalize action ordering for N participants
- expand observation model for multi-participant visibility
- preserve replayability and deterministic transition semantics

## 11. Verification Strategy

Validation should confirm:

- deterministic turn ordering under fixed input
- reproducible histories
- engine / player separation
- correct fallback for incomplete or invalid actions
- battle result shape and rating consistency
- stable TUI behavior as a display-only surface

## 13. Turn-Resolution Semantics

The engine resolves each turn deterministically using the following model:

### 13.1 Pre-turn state check

Before action collection, check for in-progress fast moves from the previous turn:
- If both participants have fast moves in-progress, only the turn clock advances (decrement durations)
- No further actions are processed; the turn is a clock-only progression

### 13.2 Action and decision collection

For each participant, collect one input at the start of the turn:

1. **Action request**: move choice (fast move, charged move, swap, or wait)

Shield decisions are **not** collected at turn start. Instead, they are requested **during turn resolution** when a charged move is about to be applied to a target. The shield decision is asynchronous and does not advance the turn counter—it occurs within the turn resolution phase.

### 13.3 Action execution order

Execute actions in this priority order:
1. **Swaps** (all swaps execute first, no damage yet)
2. **Charged moves** (ordered by CMP: higher attack stat goes first; ties broken left-to-right by participant)
3. **Fast moves** (all queued for damage application at end-of-turn)

### 13.4 Charged move resolution with CMP cascade and shield decision

For each charged move in CMP order:
- If opponent has an in-progress fast move, complete it immediately (damage still applies if attacker survives)
- **Request shield decision from defender** (asynchronous, within the turn, does not advance turn counter)
- Calculate base damage
- Apply shield if defender chose to shield (reduces damage to 1, consumes one shield)
- Apply damage to defender
- Apply stat changes and buffs/debuffs
- **Cascade check**: if defender faints:
  - Cancel defender's in-progress fast move (damage never applies)
  - Cancel defender's queued charged move (does not fire)
  - Trigger forced swap logic (see section 13.6)

### 13.5 Fast move resolution

For fast moves in queue:
- Only queue damage if source is NOT fainted (killed by opponent's charged move)
- Start or continue fast move duration (1+ turns)
- Queued damage applies at end-of-turn

### 13.6 End-of-turn resolution

1. Apply all queued fast move damage
2. Apply all energy gains (cannot be used until next turn)
3. Reset fast move cooldowns for completed moves
4. Detect any faints from queued damage
5. Process forced swaps (auto-select next available bench Pokémon)
6. Record timeline events for all actions and damage
7. Finalize snapshot for next turn

### 13.7 Key determinism rules

- **Simultaneous KOs are possible**: both participants can faint on the same turn if both survive charged moves and their fast moves both apply damage
- **Energy cannot be spent in the turn it is gained**: end-of-turn energy gain becomes available starting next turn
- **CMP controls charged move ordering**: higher attack stat charged move resolves first; if it kills, opponent's charged move does not fire
- **Forced completion of in-progress fast moves**: when opponent uses charged move, any in-progress fast move completes immediately (but only applies damage if attacker survives the charged move)
- **Left-to-right tiebreaker**: for actions of the same type with equal priority, participant-0 (left side) executes before participant-1 (right side)

### 13.8 Timeline events

The following event types are recorded during turn resolution:
- `tap interaction`: fast move starts or completes
- `charged <type>`: charged move is used (includes damage and shield status)
- `switchAvailable`: swap is available to the player (after charged move completes)
- `faint`: Pokémon faints
- `tap interaction wait`: fast move is still in-progress, waiting for completion

## 12. Accepted Design Resolutions

The following design decisions have been accepted and are now part of the implementation baseline:

### 12.1 Canonical runtime mappings

- The engine’s canonical state remains the source of truth for HP, energy, cooldowns, shields, and roster state.
- pvpoke runtime semantics are treated as compatibility targets for validation, not as a direct object mapping.
- The battle runtime must preserve the exact timing semantics for energy gain, damage application, charged attacks, and swap ordering.

### 12.2 Move timing and energy semantics

- A charged move is queued at turn start and begins at the start of the next turn.
- Damage resolves at the end of the turn.
- Energy gained during a turn is not spendable until the following turn.
- A charged move that would otherwise kill the target still resolves if it is valid and queued under the turn-order rules.

### 12.3 Shield and mitigation rules

- Shield decisions are requested during turn resolution and do not advance the turn counter.
- Shielding reduces incoming damage according to the battle rules and consumes one shield.
- The defender receives no prior knowledge of the incoming move identity or exact base damage before deciding.

### 12.4 Swap and forced-swap semantics

- Swaps resolve before damage.
- Quick swaps consume a turn.
- Forced swaps after a faint do not consume a turn.
- Swap timing remains deterministic and does not depend on network variance.
- When a Pokémon leaves the active slot and enters the bench, reset `statStages` to zero. Preserve persistent state such as HP and energy.

### 12.5 Module ownership and API boundaries

- The battle engine owns the canonical immutable turn snapshot.
- The player interface owns observation generation and action normalization.
- The resolver owns action ordering and all battle-state transitions.
- Replay/history and TUI layers are presentation or serialization concerns and must not mutate live engine state.

### 12.6 Validation strategy

- Rule-level unit tests cover move ordering, shield behavior, swap timing, and energy timing.
- Snapshot-transition tests assert exact next-state output for a fixed input set.
- Replay tests validate that generated timelines reconstruct the same battle state.
- Observation contract tests validate version compatibility and explicit visibility rules.

## 13. Remaining Open Design Questions

The blueprint still leaves the following as explicit follow-up items for implementation detail work:

- the final, exhaustive event schema for the replay timeline
- the exact serialization boundary for battle logs and turn snapshots
- any additional compatibility deltas to document when matching edge-case pvpoke behaviors
