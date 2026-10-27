use std::collections::HashSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::data::{DataCatalog, StatInitializationError};
use crate::model::{
    ActionRequest, BattleSnapshot, CombatStats, ControllerType, Cooldowns, MoveState, Observation,
    ParticipantState, PlayerAction, PokemonBuildInput, PokemonFlags, PokemonState,
    ShieldDecisionRequest, Side, StatStages, TimelineEvent,
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

pub struct TransitionObservation<'a> {
    pub sequence_index: u64,
    pub turn_before: u32,
    pub turn_after: u32,
    pub action_requests: &'a [ActionRequest],
    pub shield_decisions: &'a [ShieldDecisionRequest],
    pub events: &'a [TimelineEvent],
    pub before_snapshot: &'a BattleSnapshot,
    pub after_snapshot: &'a BattleSnapshot,
}

pub trait TransitionObserver {
    fn on_transition(&mut self, observation: TransitionObservation<'_>);
}

impl<F> TransitionObserver for F
where
    F: for<'a> FnMut(TransitionObservation<'a>),
{
    fn on_transition(&mut self, observation: TransitionObservation<'_>) {
        self(observation);
    }
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
    transition_sequence: u64,
    catalog: &'catalog DataCatalog,
}

impl<'catalog> BattleRunner<'catalog> {
    pub fn new(
        setup: &BattleSetup,
        catalog: &'catalog DataCatalog,
    ) -> Result<Self, BattleSetupError> {
        Ok(Self {
            snapshot: initialize_battle(setup, catalog)?,
            transition_sequence: 0,
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
        self.advance_turn_inner(requests, shield_decisions, None)
    }

    pub fn advance_turn_with_observer(
        &mut self,
        requests: &[ActionRequest],
        shield_decisions: &[ShieldDecisionRequest],
        observer: &mut dyn TransitionObserver,
    ) -> Result<TurnTransition, BattleRunnerError> {
        self.advance_turn_inner(requests, shield_decisions, Some(observer))
    }

    fn advance_turn_inner(
        &mut self,
        requests: &[ActionRequest],
        shield_decisions: &[ShieldDecisionRequest],
        mut observer: Option<&mut dyn TransitionObserver>,
    ) -> Result<TurnTransition, BattleRunnerError> {
        self.ensure_in_progress()?;
        let before_snapshot = observer.as_ref().map(|_| self.snapshot.clone());
        let transition = resolve_turn_with_shield_decisions(
            &self.snapshot,
            requests,
            shield_decisions,
            self.catalog,
        )?;
        self.snapshot = transition.next_snapshot.clone();
        let sequence_index = self.transition_sequence;
        self.transition_sequence = self.transition_sequence.saturating_add(1);
        if let (Some(observer), Some(before_snapshot)) = (observer.as_deref_mut(), before_snapshot)
        {
            observer.on_transition(TransitionObservation {
                sequence_index,
                turn_before: before_snapshot.turn,
                turn_after: transition.next_snapshot.turn,
                action_requests: requests,
                shield_decisions,
                events: &transition.timeline_entries,
                before_snapshot: &before_snapshot,
                after_snapshot: &transition.next_snapshot,
            });
        }
        Ok(transition)
    }

    pub fn advance_with_action_selector<F>(
        &mut self,
        observation_version: &str,
        schema_revision: u64,
        select_action: F,
    ) -> Result<TurnTransition, BattleRunnerError>
    where
        F: FnMut(&Observation) -> PlayerAction,
    {
        let requests =
            self.collect_action_requests(observation_version, schema_revision, select_action)?;
        self.advance_turn(&requests)
    }

    pub fn advance_with_action_selector_and_observer<F>(
        &mut self,
        observation_version: &str,
        schema_revision: u64,
        select_action: F,
        observer: &mut dyn TransitionObserver,
    ) -> Result<TurnTransition, BattleRunnerError>
    where
        F: FnMut(&Observation) -> PlayerAction,
    {
        let requests =
            self.collect_action_requests(observation_version, schema_revision, select_action)?;
        self.advance_turn_with_observer(&requests, &[], observer)
    }

    fn collect_action_requests<F>(
        &self,
        observation_version: &str,
        schema_revision: u64,
        mut select_action: F,
    ) -> Result<Vec<ActionRequest>, BattleRunnerError>
    where
        F: FnMut(&Observation) -> PlayerAction,
    {
        self.ensure_in_progress()?;
        self.snapshot
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
            .collect::<Result<_, ObservationError>>()
            .map_err(BattleRunnerError::from)
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
    use crate::model::{
        ActionRequest, ControllerType, IndividualValues, PlayerAction, PokemonBuildInput,
        ShieldDecision, ShieldDecisionRequest, Side,
    };
    use crate::replay::{ReplayError, ReplayRecorder};

    use super::{
        BattleRunner, BattleRunnerError, BattleSetup, BattleSetupError, ParticipantSetup,
        PokemonSetup, TransitionObservation, initialize_battle,
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

    #[test]
    fn observer_receives_selector_generated_requests_and_snapshots() {
        let catalog = DataCatalog::embedded().unwrap();
        let mut runner = BattleRunner::new(&setup(), &catalog).unwrap();
        let mut observed_requests = Vec::new();
        let mut observer = |observation: TransitionObservation<'_>| {
            assert_eq!(observation.sequence_index, 0);
            assert_eq!(observation.turn_before, 0);
            assert_eq!(observation.turn_after, 1);
            assert_eq!(observation.before_snapshot.turn, 0);
            assert_eq!(observation.after_snapshot.turn, 1);
            assert_eq!(observation.shield_decisions.len(), 0);
            assert_eq!(observation.events.len(), 2);
            observed_requests.extend(observation.action_requests.iter().cloned());
        };

        runner
            .advance_with_action_selector_and_observer(
                "1.0",
                1,
                |observation| PlayerAction::FastMove {
                    move_id: observation.self_view.active.fast_move.clone(),
                },
                &mut observer,
            )
            .unwrap();

        assert_eq!(observed_requests.len(), 2);
        assert_eq!(observed_requests[0].participant_id, "left");
        assert_eq!(observed_requests[1].participant_id, "right");
    }

    #[test]
    fn observer_receives_no_turn_forced_switch_transitions_in_sequence() {
        let catalog = DataCatalog::embedded().unwrap();
        let mut runner = BattleRunner::new(&setup(), &catalog).unwrap();
        let mut bench_pokemon = runner.snapshot.participants[0].team[0].clone();
        bench_pokemon.slot = 1;
        bench_pokemon.pokemon_id = "left-bench".to_owned();
        bench_pokemon.is_active = false;
        runner.snapshot.participants[0].team.push(bench_pokemon);
        runner
            .snapshot
            .pending_forced_swaps
            .push(crate::model::PendingForcedSwap {
                participant_id: "left".to_owned(),
                options: vec![1],
            });
        let request = ActionRequest {
            participant_id: "left".to_owned(),
            turn: 0,
            action: PlayerAction::Swap { target_slot: 1 },
        };
        let mut observed = Vec::new();
        let mut observer = |observation: TransitionObservation<'_>| {
            observed.push((
                observation.sequence_index,
                observation.turn_before,
                observation.turn_after,
                observation.events.len(),
                observation.shield_decisions.len(),
            ));
        };
        let shield_decision = ShieldDecisionRequest {
            participant_id: "right".to_owned(),
            turn: 0,
            decision: ShieldDecision::NoShield,
        };

        runner
            .advance_turn_with_observer(&[request], &[shield_decision], &mut observer)
            .unwrap();
        runner
            .advance_turn_with_observer(&[], &[], &mut observer)
            .unwrap();

        assert_eq!(observed, vec![(0, 0, 0, 1, 1), (1, 0, 1, 0, 0)]);
        assert_eq!(runner.snapshot().participants[0].active_pokemon_index, 1);
    }

    #[test]
    fn resolver_failure_does_not_notify_observer_or_advance_sequence() {
        let catalog = DataCatalog::embedded().unwrap();
        let mut runner = BattleRunner::new(&setup(), &catalog).unwrap();
        runner.snapshot.participants[1].team[0].combat_stats.defense = 0.0;
        runner.snapshot.participants[0].team[0]
            .move_state
            .fast_move_turns_remaining = 1;
        let mut notifications = 0;
        let mut observer = |_: TransitionObservation<'_>| notifications += 1;

        assert!(
            runner
                .advance_turn_with_observer(&[], &[], &mut observer)
                .is_err()
        );
        assert_eq!(notifications, 0);
        assert_eq!(runner.transition_sequence, 0);
    }

    #[test]
    fn replay_recorder_stores_compact_events_and_replays_with_catalog_revision() {
        let catalog = DataCatalog::embedded().unwrap();
        let mut runner = BattleRunner::new(&setup(), &catalog).unwrap();
        let initial_snapshot = runner.snapshot().clone();
        let mut recorder = ReplayRecorder::new(initial_snapshot, "embedded-test-v1").unwrap();

        for _ in 0..3 {
            runner
                .advance_with_action_selector_and_observer(
                    "1.0",
                    1,
                    |observation| PlayerAction::FastMove {
                        move_id: observation.self_view.active.fast_move.clone(),
                    },
                    &mut recorder,
                )
                .unwrap();
        }

        let replay = recorder.finish();
        let json = serde_json::to_value(&replay).unwrap();
        assert_eq!(json["initialSnapshot"]["rngState"], 42);
        assert_eq!(json["steps"].as_array().unwrap().len(), 3);
        assert!(json["steps"][0].get("events").is_some());
        assert!(json["steps"][0].get("resultingSnapshot").is_none());

        let serialized = serde_json::to_string(&replay).unwrap();
        let deserialized: crate::replay::BattleReplay = serde_json::from_str(&serialized).unwrap();
        assert_eq!(
            deserialized.replay(&catalog, "embedded-test-v1").unwrap(),
            *runner.snapshot()
        );
        assert!(matches!(
            deserialized.replay(&catalog, "different-catalog"),
            Err(ReplayError::CatalogRevisionMismatch { .. })
        ));

        let mut changed_rules = deserialized.clone();
        changed_rules.rules_revision = "old-rules".to_owned();
        assert!(matches!(
            changed_rules.replay(&catalog, "embedded-test-v1"),
            Err(ReplayError::RulesRevisionMismatch { .. })
        ));

        let mut changed_events = deserialized;
        changed_events.steps[0].events.clear();
        assert!(matches!(
            changed_events.replay(&catalog, "embedded-test-v1"),
            Err(ReplayError::EventMismatch { step_index: 0 })
        ));
    }
}
