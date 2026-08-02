# Project Specification

This file describes the overall design and plan for the Pokemon Go Battle Simulation components.

## Engine

Housed in the `engine/` folder, the Engine executes the event loop that simulates a Pokémon GO battle as a discrete-turn, deterministic state machine. A battle consists of two players, each controlling one active Pokémon and up to two bench Pokémon. The engine advances through turns by collecting each player’s intent, resolving action ordering and timing, then applying the resulting combat state for the turn.

### Structure

* Each battle has exactly two players.
* Each player controls a team of one to three Pokémon.
* One Pokémon is active at a time; the remaining team members are on the bench.
* Each Pokémon has current HP, energy, cooldown state, buff/debuff state, and move availability.
* Battle state is tracked over discrete turns, not continuous real time.
* The simulation uses a fixed turn cadence in which damage, energy gain, and state transitions resolve in a consistent order.

### Turn Model

* A turn is the unit of battle progression.
* Each player may submit one action per turn.
* Fast moves operate on a fixed cadence and are the default source of damage and energy generation.
* Charged moves may be queued or triggered when enough energy is stored, but their execution timing is standardized.
* Damage from fast and charged attacks resolves at the end of the turn in which the attack sequence is completed.
* Energy gains are awarded at the same time damage resolves.

### Actions

A player may take the following actions each turn: fast move, charged move, swap, wait.

* Fast move: a regular attack that deals damage and generates energy on a fixed cadence. A fast move occupies the active attack sequence until its duration has elapsed.
* Charged move: a heavier attack that consumes stored energy. If the player does not have enough energy, the action falls back to the fast move behavior for that turn.
* Swap: switches the active Pokémon with one of the bench Pokémon. A voluntary quick swap costs one turn. A forced swap caused by a fainted active Pokémon does not cost a turn.
* Wait: a no-op that advances the turn without changing the active attack or team state.

### Resolution Rules

* The engine resolves the action queue by priority and turn order, then applies the resulting state transitions.
* Swaps resolve before incoming damage for the same turn.
* When a Pokémon faints, the next active slot is selected from the remaining bench Pokémon, and the battle continues immediately.
* Charged attacks begin at the start of the next turn after they are triggered, then resolve before any fast move that finishes during the charged sequence.
* If two fast attacks complete on the same turn, their damage is treated as simultaneous and ties are preserved.
* A fast move that finishes while a charged move sequence is in progress is applied only after the charged move sequence completes.

### Energy and Cooldown Behavior

* Fast moves generate energy into a stored energy pool that is capped at 100.
* Charged moves consume energy from that pool.
* Each move has an internal cooldown or timing window that determines when it can be used again.
* The engine tracks per-turn cooldowns and ensures that a move cannot be performed before its cooldown has expired.

### End-of-Battle Conditions

* The battle continues until one side has no remaining usable Pokémon or until the combat loop reaches a safety cutoff.
* If both players are eliminated, the battle is recorded as a double knockout.
* If only one side is eliminated, the remaining side is the winner.
* The engine should be robust against invalid action sequences and should terminate safely if the simulation exceeds its maximum time or turn budget.
