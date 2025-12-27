# Project Planning

This file describes the overall design and plan for the Pokemon Go Battle Simulation components.

## Engine

Housed in the `engine/` folder, the Engine executes the event loop that simulates each turn of a Pokemon Go Battle.  

### Structure

* Each battle has two players, each of which may provide an action each turn.
* Each player has a battle team of one to three Pokemon
* Each player has one active Pokemon, which performs the actions, and up to two bench Pokemon
* When a Pokemon runs out of 

### Actions

A player may take the following actions each turn: fast move, charge move, wait, swap.

* Fast moves have a duration of one or more turns and no other action can be taken until the duration has been completed.  
* Fast moves generate energy that is stored (to a maximum of 100) for use by charge moves
* Charge moves consume stored energy, or perform the fast move if not enough energy is stored yet
* Swap switches the current active Pokemon with one of the bench Pokemon
* If there are no bench pokemon, then swap performs a fast move
* Wait is essentially a no-op that allows the turn to pass with no other action
