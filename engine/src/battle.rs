use std::collections::HashSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::data::{DataCatalog, StatInitializationError};
use crate::model::{
    ActionRequest, BattleSnapshot, CombatStats, ControllerType, Cooldowns, MoveState, Observation,
    ParticipantState, PlayerAction, PokemonBuildInput, PokemonFlags, PokemonState,
    ShieldDecisionRequest, Side, StatStages,
};
use crate::observations::{ObservationError, build_observation};
use crate::transition::{TransitionError, TurnTransition, resolve_turn_with_shield_decisions};

#[derive(Clone, Debug, PartialEq)]
pub struct BattleSetup {
    pub battle_id: String,
    pub rng_seed: u64,
    pub participants: [ParticipantSetup; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParticipantSetup {
    pub participant_id: String,
    pub controller_type: ControllerType,
    pub shields_remaining: u8,
    pub team: Vec<PokemonSetup>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PokemonSetup {
    pub pokemon_id: String,
    pub build: PokemonBuildInput,
    pub fast_move: String,
    pub charged_moves: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BattleSetupError {
    EmptyBattleId,
    EmptyParticipantId,
    DuplicateParticipantId,
    EmptyTeam(String),
    EmptyPokemonId(String),
    DuplicatePokemonId(String),
    UnknownSpecies(String),
    StatInitialization {
        species_id: String,
        error: StatInitializationError,
    },
    UnknownMove(String),
    InvalidFastMove {
        species_id: String,
        move_id: String,
    },
    InvalidChargedMove {
        species_id: String,
        move_id: String,
    },
}

impl Display for BattleSetupError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyBattleId => formatter.write_str("battle ID cannot be empty"),
            Self::EmptyParticipantId => formatter.write_str("participant ID cannot be empty"),
            Self::DuplicateParticipantId => formatter.write_str("participant IDs must be unique"),
            Self::EmptyTeam(participant_id) => {
                write!(
                    formatter,
                    "participant {participant_id} must have a Pokemon"
                )
            }
            Self::EmptyPokemonId(participant_id) => {
                write!(
                    formatter,
                    "participant {participant_id} has an empty Pokemon ID"
                )
            }
            Self::DuplicatePokemonId(pokemon_id) => {
                write!(formatter, "duplicate Pokemon ID: {pokemon_id}")
            }
            Self::UnknownSpecies(species_id) => {
                write!(formatter, "unknown Pokemon species: {species_id}")
            }
            Self::StatInitialization { species_id, error } => {
                write!(formatter, "cannot initialize {species_id}: {error}")
            }
            Self::UnknownMove(move_id) => write!(formatter, "unknown move: {move_id}"),
            Self::InvalidFastMove {
                species_id,
                move_id,
            } => {
                write!(formatter, "{move_id} is not a fast move for {species_id}")
            }
            Self::InvalidChargedMove {
                species_id,
                move_id,
            } => {
                write!(
                    formatter,
                    "{move_id} is not a charged move for {species_id}"
                )
            }
        }
    }
}

impl Error for BattleSetupError {}

pub fn initialize_battle(
    setup: &BattleSetup,
    catalog: &DataCatalog,
) -> Result<BattleSnapshot, BattleSetupError> {
    if setup.battle_id.trim().is_empty() {
        return Err(BattleSetupError::EmptyBattleId);
    }

    let mut participant_ids = HashSet::new();
    let mut pokemon_ids = HashSet::new();
    let mut participants = Vec::with_capacity(2);

    for (participant_index, participant_setup) in setup.participants.iter().enumerate() {
        if participant_setup.participant_id.trim().is_empty() {
            return Err(BattleSetupError::EmptyParticipantId);
        }
        if !participant_ids.insert(participant_setup.participant_id.as_str()) {
            return Err(BattleSetupError::DuplicateParticipantId);
        }
        if participant_setup.team.is_empty() {
            return Err(BattleSetupError::EmptyTeam(
                participant_setup.participant_id.clone(),
            ));
        }

        let mut team = Vec::with_capacity(participant_setup.team.len());
        for (slot, pokemon_setup) in participant_setup.team.iter().enumerate() {
            if pokemon_setup.pokemon_id.trim().is_empty() {
                return Err(BattleSetupError::EmptyPokemonId(
                    participant_setup.participant_id.clone(),
                ));
            }
            if !pokemon_ids.insert(pokemon_setup.pokemon_id.as_str()) {
                return Err(BattleSetupError::DuplicatePokemonId(
                    pokemon_setup.pokemon_id.clone(),
                ));
            }

            let species_id = &pokemon_setup.build.species_id;
            let species = catalog
                .pokemon_definition(species_id)
                .ok_or_else(|| BattleSetupError::UnknownSpecies(species_id.clone()))?;
            if !species.fast_moves.contains(&pokemon_setup.fast_move) {
                return Err(BattleSetupError::InvalidFastMove {
                    species_id: species_id.clone(),
                    move_id: pokemon_setup.fast_move.clone(),
                });
            }
            if catalog.move_definition(&pokemon_setup.fast_move).is_none() {
                return Err(BattleSetupError::UnknownMove(
                    pokemon_setup.fast_move.clone(),
                ));
            }

            let mut selected_charged_moves = HashSet::new();
            for move_id in &pokemon_setup.charged_moves {
                if !species.charged_moves.contains(move_id)
                    || catalog.charged_move_cost(move_id).is_none()
                {
                    if catalog.move_definition(move_id).is_none() {
                        return Err(BattleSetupError::UnknownMove(move_id.clone()));
                    }
                    return Err(BattleSetupError::InvalidChargedMove {
                        species_id: species_id.clone(),
                        move_id: move_id.clone(),
                    });
                }
                if !selected_charged_moves.insert(move_id) {
                    return Err(BattleSetupError::InvalidChargedMove {
                        species_id: species_id.clone(),
                        move_id: move_id.clone(),
                    });
                }
            }

            let initialized = catalog
                .initialize_stats(&pokemon_setup.build)
                .map_err(|error| BattleSetupError::StatInitialization {
                    species_id: species_id.clone(),
                    error,
                })?;
            team.push(PokemonState {
                slot,
                pokemon_id: pokemon_setup.pokemon_id.clone(),
                species: species_id.clone(),
                is_active: slot == 0,
                current_hp: initialized.max_hp,
                max_hp: initialized.max_hp,
                combat_stats: CombatStats {
                    attack: initialized.combat_stats.attack,
                    defense: initialized.combat_stats.defense,
                },
                stat_stages: StatStages::default(),
                energy: 0,
                pending_energy_gain: 0,
                cooldowns: Cooldowns {
                    fast_move_ms: 0,
                    charged_move_ms: 0,
                    swap_ms: 0,
                },
                move_state: MoveState {
                    fast_move: pokemon_setup.fast_move.clone(),
                    fast_move_turns_remaining: 0,
                    charged_moves: pokemon_setup.charged_moves.clone(),
                    charged_ready: false,
                    pending_charge: false,
                },
                flags: PokemonFlags {
                    fainted: false,
                    shielded: false,
                    shadow: pokemon_setup.build.shadow,
                },
            });
        }

        participants.push(ParticipantState {
            participant_id: participant_setup.participant_id.clone(),
            controller_type: participant_setup.controller_type.clone(),
            side: if participant_index == 0 {
                Side::Left
            } else {
                Side::Right
            },
            active_pokemon_index: 0,
            team,
            shields_remaining: participant_setup.shields_remaining,
            switch_cooldown_turns: 0,
        });
    }

    Ok(BattleSnapshot {
        battle_id: setup.battle_id.clone(),
        turn: 0,
        rng_state: setup.rng_seed,
        participants,
        pending_charged_moves: Vec::new(),
        pending_forced_swaps: Vec::new(),
        timeline: Vec::new(),
        metadata: Default::default(),
    })
}

#[derive(Debug)]
pub enum BattleRunnerError {
    Finished,
    Observation(ObservationError),
    Transition(TransitionError),
}

impl Display for BattleRunnerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Finished => formatter.write_str("battle is already finished"),
            Self::Observation(error) => Display::fmt(error, formatter),
            Self::Transition(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for BattleRunnerError {}

impl From<ObservationError> for BattleRunnerError {
    fn from(error: ObservationError) -> Self {
        Self::Observation(error)
    }
}

impl From<TransitionError> for BattleRunnerError {
    fn from(error: TransitionError) -> Self {
        Self::Transition(error)
    }
}

pub struct BattleRunner<'catalog> {
    snapshot: BattleSnapshot,
    catalog: &'catalog DataCatalog,
}

impl<'catalog> BattleRunner<'catalog> {
    pub fn new(
        setup: &BattleSetup,
        catalog: &'catalog DataCatalog,
    ) -> Result<Self, BattleSetupError> {
        Ok(Self {
            snapshot: initialize_battle(setup, catalog)?,
            catalog,
        })
    }

    pub fn snapshot(&self) -> &BattleSnapshot {
        &self.snapshot
    }

    pub fn is_finished(&self) -> bool {
        self.snapshot.participants.len() != 2
            || self.snapshot.participants.iter().any(|participant| {
                !participant
                    .team
                    .iter()
                    .any(|pokemon| pokemon.current_hp > 0 && !pokemon.flags.fainted)
            })
    }

    pub fn advance_turn(
        &mut self,
        requests: &[ActionRequest],
    ) -> Result<TurnTransition, BattleRunnerError> {
        self.advance_turn_with_shield_decisions(requests, &[])
    }

    pub fn advance_turn_with_shield_decisions(
        &mut self,
        requests: &[ActionRequest],
        shield_decisions: &[ShieldDecisionRequest],
    ) -> Result<TurnTransition, BattleRunnerError> {
        self.ensure_in_progress()?;
        let transition = resolve_turn_with_shield_decisions(
            &self.snapshot,
            requests,
            shield_decisions,
            self.catalog,
        )?;
        self.snapshot = transition.next_snapshot.clone();
        Ok(transition)
    }

    pub fn advance_with_action_selector<F>(
        &mut self,
        observation_version: &str,
        schema_revision: u64,
        mut select_action: F,
    ) -> Result<TurnTransition, BattleRunnerError>
    where
        F: FnMut(&Observation) -> PlayerAction,
    {
        self.ensure_in_progress()?;
        let requests: Vec<_> = self
            .snapshot
            .participants
            .iter()
            .map(|participant| {
                let observation = build_observation(
                    &self.snapshot,
                    &participant.participant_id,
                    observation_version,
                    schema_revision,
                )?;
                Ok(ActionRequest {
                    participant_id: participant.participant_id.clone(),
                    turn: self.snapshot.turn,
                    action: select_action(&observation),
                })
            })
            .collect::<Result<_, ObservationError>>()?;
        self.advance_turn(&requests)
    }

    fn ensure_in_progress(&self) -> Result<(), BattleRunnerError> {
        if self.is_finished() {
            return Err(BattleRunnerError::Finished);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::data::DataCatalog;
    use crate::model::{ControllerType, IndividualValues, PlayerAction, PokemonBuildInput, Side};

    use super::{
        BattleRunner, BattleRunnerError, BattleSetup, BattleSetupError, ParticipantSetup,
        PokemonSetup, initialize_battle,
    };

    fn setup() -> BattleSetup {
        BattleSetup {
            battle_id: "setup-test".to_owned(),
            rng_seed: 42,
            participants: [
                participant("left", "bulbasaur", "VINE_WHIP", "SEED_BOMB"),
                participant("right", "squirtle", "BUBBLE", "AQUA_TAIL"),
            ],
        }
    }

    fn participant(
        participant_id: &str,
        species_id: &str,
        fast_move: &str,
        charged_move: &str,
    ) -> ParticipantSetup {
        ParticipantSetup {
            participant_id: participant_id.to_owned(),
            controller_type: ControllerType::Scripted,
            shields_remaining: 2,
            team: vec![PokemonSetup {
                pokemon_id: format!("{participant_id}-pokemon"),
                build: PokemonBuildInput {
                    species_id: species_id.to_owned(),
                    level: 20.0,
                    ivs: IndividualValues {
                        attack: 10,
                        defense: 10,
                        hp: 10,
                    },
                    shadow: false,
                },
                fast_move: fast_move.to_owned(),
                charged_moves: vec![charged_move.to_owned()],
            }],
        }
    }

    #[test]
    fn initialization_builds_a_seeded_ordered_one_v_one_snapshot() {
        let catalog = DataCatalog::embedded().unwrap();
        let snapshot = initialize_battle(&setup(), &catalog).unwrap();

        assert_eq!(snapshot.battle_id, "setup-test");
        assert_eq!(snapshot.turn, 0);
        assert_eq!(snapshot.rng_state, 42);
        assert_eq!(snapshot.participants[0].side, Side::Left);
        assert_eq!(snapshot.participants[1].side, Side::Right);
        assert_eq!(
            snapshot.participants[0].team[0].current_hp,
            snapshot.participants[0].team[0].max_hp
        );
        assert_eq!(
            snapshot.participants[0].team[0].move_state.fast_move,
            "VINE_WHIP"
        );
        assert_eq!(
            snapshot.participants[1].team[0].move_state.charged_moves,
            ["AQUA_TAIL"]
        );
        assert!(snapshot.timeline.is_empty());
    }

    #[test]
    fn initialization_rejects_moves_unavailable_to_the_species() {
        let catalog = DataCatalog::embedded().unwrap();
        let mut invalid_setup = setup();
        invalid_setup.participants[0].team[0].fast_move = "BUBBLE".to_owned();

        assert!(matches!(
            initialize_battle(&invalid_setup, &catalog),
            Err(BattleSetupError::InvalidFastMove { .. })
        ));
    }

    #[test]
    fn runner_collects_observation_based_actions_and_advances_deterministically() {
        let catalog = DataCatalog::embedded().unwrap();
        let setup = setup();
        let mut first = BattleRunner::new(&setup, &catalog).unwrap();
        let mut second = BattleRunner::new(&setup, &catalog).unwrap();
        let initial_turn = first.snapshot().turn;

        let first_transition = first
            .advance_with_action_selector("1.0", 1, |observation| PlayerAction::FastMove {
                move_id: observation.self_view.active.fast_move.clone(),
            })
            .unwrap();
        let second_transition = second
            .advance_with_action_selector("1.0", 1, |observation| PlayerAction::FastMove {
                move_id: observation.self_view.active.fast_move.clone(),
            })
            .unwrap();

        assert_eq!(initial_turn, 0);
        assert_eq!(first.snapshot().turn, 1);
        assert_eq!(
            first_transition.next_snapshot,
            second_transition.next_snapshot
        );
        assert_eq!(first.snapshot(), &first_transition.next_snapshot);
        assert_eq!(first_transition.timeline_entries.len(), 2);
    }

    #[test]
    fn runner_stops_after_a_battle_has_no_living_team_members() {
        let catalog = DataCatalog::embedded().unwrap();
        let mut runner = BattleRunner::new(&setup(), &catalog).unwrap();
        runner.snapshot.participants[0].team[0].current_hp = 0;
        runner.snapshot.participants[0].team[0].flags.fainted = true;

        assert!(runner.is_finished());
        assert!(matches!(
            runner.advance_turn(&[]),
            Err(BattleRunnerError::Finished)
        ));
    }
}
