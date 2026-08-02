# Engineering Specification: Pokémon GO Battle Simulator

Status: Draft for clean-room reimplementation

## 1. Purpose

This document describes the internal battle simulation logic used by the repository’s JavaScript engine. It is intentionally scoped to the simulation runtime and the battle/ranking orchestration layers. It excludes UI presentation, browser-specific rendering, and user interface-only helpers.

The goal is to provide enough implementation detail to support a faithful, platform-independent reimplementation of the simulator’s combat execution and ranking behavior.

## 2. Scope

In scope:

- Battle state management
- Pokémon stat initialization and CP scaling
- Fast move and charged move action selection
- Damage calculation and type effectiveness
- Shield usage, priority resolution, and action ordering
- Battle timeline construction
- Ranking/orchestration behavior that repeatedly runs simulated battles

Out of scope:

- DOM/UI rendering
- Interface controls and page layout
- Training/game-emulation front-end features unrelated to the core battle engine

## 3. Terminology

- Battle: a two-side matchup between two Pokémon.
- Pokémon: a single combatant with base stats, IVs, current HP, energy, and move pools.
- Fast move: a move that generates energy and is used on a fixed attack cadence.
- Charged move: a move that costs energy and delivers a heavier attack.
- Shield: a defensive mitigation applied against a charged move; damage is reduced to 1 instead of the full damage amount.
- Priority: a numeric ordering tie-breaker used when actions occur on the same turn.
- Mode:
  - simulate: automated simulation using the engine decision logic
  - emulate: UI-driven or AI-driven battle playback with animation and control flow

## 4. Core Runtime Components

### 4.1 Data source

The simulator relies on a central game data object loaded from the repository data package. The runtime expects:

- Pokémon definitions with species metadata, base stats, move lists, tags, and default IV profile
- Move definitions with power, type, energy cost, energy gain, cooldown, buffs/debuffs, and special flags
- Cups/formats and scenario definitions used by ranking

Primary runtime object:

- `GameMaster` singleton

### 4.2 Entity models

1. `Pokemon`
   - Represents a Pokémon instance in battle.
   - Stores battle stats, current HP, energy, shields, cooldown, buffs, and selected moves.
   - Initializes move pools from the loaded GameMaster data.

2. `Player`
   - Represents a controller side in a battle.
   - Tracks shields, switch timer, team roster, and AI/strategy references.

3. `Battle`
   - Manages turns, state transitions, actions, and timeline output.
   - Composes the engine loop by repeatedly issuing actions and applying moves.

## 5. Game Data Model

### 5.1 Pokémon data

Each Pokémon entry must provide:

- `speciesId`
- `speciesName`
- `baseStats.atk`
- `baseStats.def`
- `baseStats.hp`
- `types`
- `fastMoves`
- `chargedMoves`
- `legacyMoves`, `eliteMoves`, and flags as needed
- optional `formChange`, `aliasId`, `family`, `tags`, and `defaultIVs`

### 5.2 Move data

Each move entry must provide:

- `moveId`
- `name`
- `type`
- `power`
- `energy`
- `energyGain`
- `cooldown` in milliseconds or a turn-equivalent basis
- optional `buffs`, `buffTarget`, `selfBuffing`, `selfDebuffing`, `charge`, `chance`, `legacy`, `elite`

## 6. Stat Initialization and CP Scaling

### 6.1 Base stat formula

The simulator computes combat stats from CP level scaling, IVs, and a CPM value.

The effective stats used in simulation are:

- `stats.atk = cpm × (baseStats.atk + ivs.atk)`
- `stats.def = cpm × (baseStats.def + ivs.def)`
- `stats.hp = max(floor(cpm × (baseStats.hp + ivs.hp)), 10)`

A special case exists for `Shedinja`, which is forced to `10` HP.

### 6.2 CP calculation

The runtime uses the standard CP approximation function:

$$
CP = \left\lfloor \frac{(atk + iv_{atk}) \sqrt{(def + iv_{def})} \sqrt{(hp + iv_{hp})} \cdot cpm^2}{10} \right\rfloor
$$

### 6.3 CPM lookup

CPM is derived from the Pokémon’s level using a lookup table. The simulator uses level-based CPM values and may scale to league target CP.

### 6.4 Initialization modes

The Pokémon constructor/initialization supports several ways to derive a relevant IV/level setup:

- `gamemaster`: use default IVs for the configured CP cap
- `scale`: solve for an IV combination to hit or approximate the target CP
- `maximize`: maximize a selected stat and reinitialize the Pokémon

The simulator uses this to assign the actual stat values before a battle begins.

## 7. Damage Model

### 7.1 Constants

The damage engine uses the following multiplicative constants:

- `BONUS = 1.2999999523162841796875`
- `SUPER_EFFECTIVE = 1.60000002384185791015625`
- `RESISTED = 0.625`
- `DOUBLE_RESISTED = 0.390625`
- `STAB = 1.2000000476837158203125`
- `SHADOW_ATK = 1.2`
- `SHADOW_DEF = 0.83333331`

### 7.2 Type effectiveness

Type effectiveness is computed by comparing move type against a target’s defensive types:

- Super effective: multiply by `1.6`
- Resisted: multiply by `0.625`
- Double resisted: multiply by `0.390625`
- Normal: multiply by `1.0`

The simulator stores the calculated effectiveness array per Pokémon for the battle context.

### 7.3 Damage formula

The core static damage formula is:

$$
\text{damage} = \left\lfloor \text{power} \cdot \text{stab} \cdot \frac{\text{attackStat}}{\text{defenseStat}} \cdot \text{effectiveness} \cdot \text{chargeMultiplier} \cdot 0.5 \cdot \text{BONUS} \right\rfloor + 1
$$

Where:

- `power` is the move’s base power
- `stab` is `1.2` if the move type matches either of the attacker’s types, otherwise `1.0`
- `attackStat` is the attacker’s effective attack stat
- `defenseStat` is the defender’s effective defense stat
- `effectiveness` is the type effectiveness multiplier
- `chargeMultiplier` is normally `1.0`, but can be altered for charged move charge context in emulation mode

### 7.4 Variant formulas

The code also includes derived formulas for:

- `damageByStats`: direct damage given explicit attack/defense values and effectiveness
- `breakpoint`: solve for attack required to reach a target damage value
- `bulkpoint`: solve for defense required to reach a target damage value

These support move comparisons, meta analysis, and UI diagnostics but are not part of the base battle loop.

## 8. Buff and Debuff Model

### 8.1 Stat buff stages

The simulator tracks attack and defense stat buffs as integer stages. The multiplier is:

- if buff stage is positive: `((buffDivisor + stage) / buffDivisor)`
- if buff stage is negative: `buffDivisor / (buffDivisor - stage)`

The simulator retrieves the active buff multiplier from the current stage state or from the start-of-battle stage state depending on context.

### 8.2 Buff application

Moves may apply buffs or debuffs to self or opponent. The simulator records the current buff state before and after the move and resets them if needed.

Relevant behaviors:

- self-buffing charged moves may increase the attacker’s attack or defense
- self-debuffing moves may lower the attacker’s own stats
- opponent debuffing moves may reduce defender stats

### 8.3 Shadow mechanics

Shadow Pokémon apply multipliers to attack and defense:

- `shadowAtkMult = 1.2`
- `shadowDefMult = 0.83333331`

## 9. Battle Execution Model

### 9.1 Initialization

When a battle is created:

1. A new `Battle` object is created.
2. `GameMaster` is accessed for data lookup.
3. Two Pokémon are assigned to slots `0` and `1` using `setNewPokemon`.
4. Pokémon are reset and initialized for the battlefield context.
5. Starting HP, energy, shields, and stat state are established.

### 9.2 Battle loop

The main loop is:

1. `start()` resets state, cooldowns, timeline, queues, and round counters.
2. `step()` executes one logical turn.
3. `simulate()` repeatedly calls `step()` until one or both Pokémon are incapacitated or a time limit is reached.

The simulation ends when:

- both Pokémon are fainted, in `battleEndMode == "both"`, or
- one Pokémon is fainted in single-win mode, or
- `time > 240000` ms (fail-safe loop cutoff)

### 9.3 Turn progression

Each `step()` progression observes the following order:

1. Reset per-turn action tracking.
2. Decrement cooldowns for both combatants.
3. Determine action for each side for the current turn.
4. Queue valid actions for processing.
5. Prioritize queued actions.
6. Execute actions in sorted order.
7. Advance time and turn count.
8. Detect fainting and record timeline events.

### 9.4 Time model

The simulator uses a virtual time domain:

- `deltaTime = 500` ms for ordinary turn progression
- `chargedMinigameTime = 10000` ms when charged moves are involved for display/priority calculations

If a turn contains no charged move, the simulator advances by `deltaTime`. If a charged move occurs, it may insert a larger display-time increment.

## 10. Action Selection

### 10.1 Action types

The battle engine supports these action types:

- `fast`
- `charged`
- `switch`
- `wait`

### 10.2 Action lifecycle

An action begins as a `TimelineAction` object and may be:

- queued for a later turn,
- prioritized against other actions,
- validated against HP, energy, cooldown, and battle state,
- executed via `processAction()`.

### 10.3 Fast move decision

When a Pokémon is eligible to act and no explicit action has been queued:

- in `simulate` mode, `ActionLogic.decideAction()` selects the next move
- if no charged move is ready or energy farming is active, the default is to fast move

A fast move is always chosen as a fallback if no other action is selected.

### 10.4 Charged move decision

A charged move is considered if:

- the Pokémon has enough energy,
- the move is not blocked by active cooldown or invalid state,
- and the action is allowed by the current tactical logic.

### 10.5 Priority semantics

Priority is used to resolve action ordering when multiple actions are available in the same turn. The engine adds priority components based on:

- base Pokémon priority,
- charged move use,
- higher attack stat,
- and special move conditions

When a charged move is used, the simulator may add a priority bonus to reflect move timing or the current turn order.

## 11. Shielding Logic

### 11.1 Shield availability

Each side begins with a quantity of shields. A shield reduces the damage of a charged move to `1` instead of the full damage amount.

### 11.2 Shield decision algorithm

The function `wouldShield()` is the decision helper used to decide whether a Pokémon should use a shield against an incoming charged move.

It evaluates the following inputs:

- attacker and defender current stats,
- damage of the incoming move,
- resulting post-move HP,
- the attacker’s fast attack damage and fast-move cadence,
- the attacker’s next possible charged move damage,
- whether the move self-buffs or self-debuffs.

### 11.3 Heuristic conditions

A shield is favored when:

- the move would leave the defender at or below the estimated future cycle damage threshold,
- the attacker has a lethal or near-lethal follow-up charged move,
- the move is self-debuffing and would likely punish the defender,
- or the attacker is configured with shield bait behavior (`baitShields == 2`).

### 11.4 Randomized simulation mode

In randomized battles, the engine uses weighted options rather than absolute booleans. The helper returns a `shieldWeight` and `noShieldWeight` pair and the caller chooses weighted randomly.

## 12. Action/Move State and Battle State

The internal decision layer uses a dynamic-programming style state object named `BattleState` with the following fields:

- `energy`: attacker energy state
- `oppHealth`: opponent HP state
- `turn`: current simulated turn number
- `oppShields`: opponent shields state
- `moves`: list of already-used moves in the path
- `buffs`: attack buff state
- `chance`: probability weight for that path

This structure is used to explore favorable state transitions and estimate outcomes for decision making.

## 13. Form Changes and Special Mechanics

Some Pokémon have special form transitions triggered by charged moves or shield usage.

Examples:

- `aegislash_shield` uses the blade form’s attack stat when a charged move is used.
- form changes can trigger:
  - before charged move activation,
  - after shield application,
  - or on switch reset.

The battle engine uses `changeForm()` to alter the active form and recompute the relevant stat context.

## 14. Pokémon State Reset and Reuse

The simulation reset semantics are important:

- `reset()` returns a Pokémon to a clean combat state for the next battle
- `fullReset()` is used for team or training state restoration
- `setStartHp()` and `setStartEnergy()` preserve initial values for later metrics
- `setStartBuffs()` returns progress to the starting buff configuration

When a Pokémon switches out, the simulator resets buff state and restores the original form when the form-change rules require it.

## 15. Battle Output and Timeline

The engine emits a timeline of events used for display and analysis. Examples include:

- `tap interaction`
- `charged <type>`
- `switchAvailable`
- `faint`
- `tap interaction wait`

The timeline records the event, the actor, the time, and the turn number. The final battle rating for each side is derived from the remaining HP and inflicted damage:

$$
\text{rating} = \left\lfloor 500 \cdot \frac{\text{opponent HP damage}}{\text{opponent max HP}} + 500 \cdot \frac{\text{self remaining HP}}{\text{self max HP}} \right\rfloor
$$

## 16. Simulation Determinism and Randomness

The simulator supports both deterministic and randomized decision modes.

- `decisionMethod == "default"` chooses the tactical action as defined by the action logic.
- `decisionMethod == "random"` introduces weighted random shield decisions.

The engine is otherwise structured as a deterministic event loop with a fixed order of state transitions and routing.

## 17. Failure Conditions and Safety Limits

The simulator contains safety safeguards:

- a hard loop cap on total elapsed time
- invalid move and invalid action checks
- invalid energy and cooldown guards
- skip logic for dead combatants or actions now impossible to resolve

If the loop exceeds the maximum duration, the battle ends without producing an invalid infinite progression.

## 18. Invariants of the Engine

A faithful implementation should preserve the following invariants:

1. Two combatants only.
2. Combat flows in discrete turns.
3. Fast moves generate energy on a fixed cadence.
4. Charged moves consume energy and may be shielded.
5. Priority and cooldown order determine which action executes first.
6. HP, shields, and energy are integer-like state values for the battle loop.
7. Damage is computed from the current effective attack and defense stats, not from a precomputed table.
8. The timeline is produced as a byproduct of execution, not as a separate simulation layer.

## Appendix A: Orchestration Layer — Ranker.js

### A.1 Role

`Ranker.js` is not the combat engine itself. It is the orchestration layer that repeatedly executes battles over many Pokémon and scenarios to build ranking datasets.

### A.2 Inputs

- league CP value
- cup / format selection
- scenario definitions
- move overrides or ranking data
- move selection mode (`auto` or `force`)

### A.3 Setup and data loading

The ranker performs the following setup:

1. Create a `Battle` object.
2. Apply the selected CP and cup.
3. Build a `pokemonList` of eligible Pokémon from `GameMaster`.
4. Build `targets` from the ranking pool.
5. Optionally load prior ranking data to preserve move recommendations.

### A.4 Ranking loop

For each Pokémon in the list, the ranker:

1. creates a `rankObj` record,
2. simulates one battle against every target,
3. records per-matchup results,
4. aggregates ratings, move usage, and counters,
5. results in a final ranking output object.

The key logic is symmetrically favorable because when shield counts and energy scenarios are identical, a battle result can be re-used to avoid duplicate simulation.

### A.5 Ranking output structure

Each rank record contains:

- `speciesId`
- `speciesName`
- `rating`
- `matches`
- `matchups`
- `counters`
- `moves`

These fields are then serialized or passed to callers for UI rendering or stored rankings.

### A.6 Scenario model

A scenario provides the battle conditions used during ranking, such as:

- shield counts for each side
- energy advantage / disadvantage settings
- specific format/rules assumptions

The ranker executes every Pokémon against all relevant targets under these scenario settings.

## Appendix B: Orchestration Layer — TeamRanker.js

### B.1 Role

`TeamRanker.js` is a specialized ranker used for team-builder-style analysis. Unlike the main ranker, it evaluates a supplied team of Pokémon and ranks other Pokémon against that team.

### B.2 Inputs

- `team`: the Pokémon team to assess against
- `cp`: combat power cap
- `cup`: active cup or format
- optional `exclusionList`
- `context`: one of `team-counters`, `team-alternatives`, `matrix`, or related evaluator modes

### B.3 Execution flow

The team ranker:

1. constructs a new `Battle` object,
2. sets the CP / cup/level cap,
3. builds a candidate Pokémon list,
4. optionally applies recommended movesets,
5. iterates over each candidate and each member of the supplied team,
6. sets shields and battle settings,
7. runs `battle.simulate()`,
8. aggregates rating and matchup scores.

### B.4 Operational differences from Ranker.js

Compared to the main ranker:

- it focuses on a provided team rather than a full league-wide ranking list,
- it may compute multiple shield scenarios and average them,
- it may tune scores differently for team-builder counter analysis,
- and it may generate `matrix`, `counters`, or `alternatives` style ranking outputs.

### B.5 Rating aggregation inside TeamRanker

Per matchup, the team ranker computes:

- `healthRating = pokemon.hp / pokemon.stats.hp`
- `damageRating = (opponent.stats.hp - opponent.hp) / opponent.stats.hp`
- `opHealthRating = opponent.hp / opponent.stats.hp`
- `opDamageRating = (pokemon.stats.hp - pokemon.hp) / pokemon.stats.hp`

Then it combines the two sides into a resulting rating:

$$
\text{rating} = \left\lfloor (\text{healthRating} + \text{damageRating}) \cdot 500 \right\rfloor
$$

Average shield scenario results are then combined into a single score used in the final recommendation output.

### B.6 Why this layer matters for a clean-room implementation

A clean-room implementation should treat `Ranker.js` and `TeamRanker.js` as orchestration wrappers that drive the same low-level battle simulator many times under differing conditions. The battle logic itself remains in the core engine and should not be duplicated or reinterpreted via the ranking layer.

## Appendix C: Gamemaster Data Schemas Required for Simulation

This appendix defines the minimum data contract expected from the gamemaster JSON payload that must be loaded into the simulator runtime.

### C.1 Root gamemaster schema

The root object must contain the following top-level fields:

```json
{
  "id": "string",
  "title": "string",
  "settings": { ... },
  "rankingScenarios": [ ... ],
  "pokemon": [ ... ],
  "moves": [ ... ],
  "formats": [ ... ],
  "cups": [ ... ]
}
```

Required for the simulation core:

- `settings`
- `rankingScenarios`
- `pokemon`
- `moves`

Optional but used by the orchestration/ranking layer:

- `formats`
- `cups`
- `pokemonTags`, `pokemonTraits`, `pokemonRegions`, and other metadata arrays used by filters and search helpers

### C.2 `settings` schema

```json
{
  "partySize": 3,
  "maxBuffStages": 4,
  "buffDivisor": 4
}
```

Semantics:

- `partySize`: size of a roster / team in training-style workflows
- `maxBuffStages`: maximum positive or negative buff stage magnitude
- `buffDivisor`: base divisor used in buff-multiplier computations

### C.3 `rankingScenarios` schema

```json
[
  {
    "slug": "string",
    "shields": [0 | 1, 0 | 1],
    "energy": [number, number]
  }
]
```

Semantics:

- `slug`: scenario identifier
- `shields`: shield allocation for side 0 and side 1
- `energy`: energy advantage or disadvantage settings for side 0 and side 1

The ranking layer uses these scenarios as repeatable battle conditions when generating outputs.

### C.4 `pokemon` entity schema

Each Pokémon entry in `pokemon` must be shaped like:

```json
{
  "dex": 1,
  "speciesName": "string",
  "speciesId": "string",
  "aliasId": "string?",
  "baseStats": {
    "atk": "integer",
    "def": "integer",
    "hp": "integer"
  },
  "types": ["string", "string?"],
  "fastMoves": ["string"],
  "chargedMoves": ["string"],
  "tags": ["string"],
  "defaultIVs": {
    "cp500": [number, number, number, number],
    "cp1500": [number, number, number, number],
    "cp2500": [number, number, number, number]
  },
  "buddyDistance": "integer",
  "thirdMoveCost": 10000 | 50000 | 75000 | 100000 | false,
  "released": "boolean",
  "family": {
    "id": "string",
    "parent": "string?",
    "evolutions": ["string"]
  },
  "levelCap": "integer?",
  "levelFloor": "integer?",
  "formChange": { ... } ,
  "legacyMoves": ["string"],
  "eliteMoves": ["string"]
}
```

Required fields for successful battle simulation:

- `dex`
- `speciesName`
- `speciesId`
- `baseStats`
- `types`
- `fastMoves`
- `chargedMoves`
- `defaultIVs`
- `released`

The following fields are strongly used by the runtime during initialization and ranking:

- `tags` for shadow, legendary, starter, and filter logic
- `family` for evolution and family-based logic
- `buddyDistance` and `thirdMoveCost` for metadata handling
- `defaultIVs` for CP-specific initial stat setup

### C.5 `moves` entity schema

Each move entry in `moves` must be shaped like:

```json
{
  "moveId": "string",
  "name": "string",
  "abbreviation": "string?",
  "type": "string",
  "power": "integer",
  "energy": "integer",
  "energyGain": "integer",
  "cooldown": "integer",
  "turns": "integer",
  "archetype": "string?",
  "buffs": [number, number]?,
  "buffTarget": "self" | "opponent" | "both"?,
  "buffApplyChance": "string?",
  "selfBuffing": "boolean?",
  "selfDebuffing": "boolean?"
}
```

Required fields for successful combat simulation:

- `moveId`
- `name`
- `type`
- `power`
- `energy`
- `energyGain`
- `cooldown`
- `turns`

Additional fields that participate in tactical logic:

- `buffs` and `buffTarget`
- `buffApplyChance`
- `selfBuffing` and `selfDebuffing`
- `archetype` used for category description and ranking metadata

### C.6 `formats` schema

A format/cup entry is not required for the low-level engine to run a single battle, but it is required for ranked or cup-scoped orchestration.

```json
{
  "id": "string",
  "name": "string",
  "title": "string",
  "cup": "string",
  "cp": "integer",
  "include": ["string"],
  "exclude": ["string"],
  "filterTargets": "boolean?",
  "levelCap": "integer?",
  "showFormat": "boolean?",
  "hideRankings": "boolean?"
}
```

The engine uses cup/format definitions to determine which Pokémon are eligible for ranking, as well as which tags or type filters are applied.

### C.7 Required simulation invariants for the loaded payload

A clean-room reimplementation must ensure that the loaded gamemaster payload satisfies these invariants:

1. Every referenced Pokémon move ID in a Pokémon record exists in the `moves` array.
2. Every move record has a valid type and energy model.
3. The battle setup can resolve default IVs for the current CP cap.
4. The chosen battle `cup` or `format` resolves to a consistent include/exclude filter set.
5. The `settings` object preserves the buff multiplier constants used by the damage model.

### C.8 Minimal load subset for a simulator-only implementation

If the objective is to reimplement only the combat engine, a minimal viable JSON load subset is:

- root `settings`
- root `rankingScenarios`
- root `pokemon`
- root `moves`

The `formats` and `cups` arrays may be omitted from the simulator-only loader if the reimplementation only handles direct battle execution and not full ranking orchestration.

## Appendix D: Recommended Reimplementation Boundary

A faithful reimplementation should split responsibilities as follows:

- Core engine:
  - `Battle`
  - `DamageCalculator`
  - `ActionLogic`
  - `Pokemon`
  - `Player`
  - `GameMaster` data access

- Orchestration:
  - `Ranker.js`
  - `TeamRanker.js`

The engine should expose a small API surface such as:

- initialize combatants
- apply CP and format settings
- set shields and starting state
- run `simulate()`
- return timeline and final battle ratings

The rankers should be implemented separately as combiners over this battle engine, not as a second combat engine.
