use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::model::{
    BattleSnapshot, Observation, ObservedPokemon, OpponentActiveObservation, OpponentObservation,
    SelfObservation, TimelineEventKind,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObservationError {
    ParticipantNotFound,
    UnsupportedParticipantCount,
    InvalidActivePokemon,
}

impl Display for ObservationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParticipantNotFound => formatter.write_str("participant not found"),
            Self::UnsupportedParticipantCount => {
                formatter.write_str("observations currently support exactly two participants")
            }
            Self::InvalidActivePokemon => formatter.write_str("active Pokemon is invalid"),
        }
    }
}

impl Error for ObservationError {}

pub fn build_observation(
    state: &BattleSnapshot,
    participant_id: &str,
    observation_version: &str,
    schema_revision: u64,
) -> Result<Observation, ObservationError> {
    if state.participants.len() != 2 {
        return Err(ObservationError::UnsupportedParticipantCount);
    }

    let self_index = state
        .participants
        .iter()
        .position(|participant| participant.participant_id == participant_id)
        .ok_or(ObservationError::ParticipantNotFound)?;
    let participant = &state.participants[self_index];
    let opponent = &state.participants[1 - self_index];
    let active = participant
        .team
        .get(participant.active_pokemon_index)
        .ok_or(ObservationError::InvalidActivePokemon)?;
    let opponent_active = opponent
        .team
        .get(opponent.active_pokemon_index)
        .ok_or(ObservationError::InvalidActivePokemon)?;

    let bench = participant
        .team
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != participant.active_pokemon_index)
        .map(|(_, pokemon)| observed_pokemon(pokemon))
        .collect();
    let forced_switch_options = state
        .pending_forced_swaps
        .iter()
        .find(|pending| pending.participant_id == participant.participant_id)
        .into_iter()
        .flat_map(|pending| pending.options.iter())
        .filter_map(|slot| {
            participant
                .team
                .iter()
                .find(|pokemon| pokemon.slot == *slot)
                .map(observed_pokemon)
        })
        .collect();
    let last_used_move = state.timeline.last().and_then(|turn| {
        turn.events
            .iter()
            .rev()
            .find(|event| {
                event.participant_id == opponent.participant_id
                    && event.move_id.is_some()
                    && (event.kind == TimelineEventKind::TapInteraction
                        || event.kind == TimelineEventKind::Charged)
            })
            .and_then(|event| event.move_id.clone())
    });

    Ok(Observation {
        observation_version: observation_version.to_owned(),
        schema_revision,
        turn: state.turn,
        self_view: SelfObservation {
            participant_id: participant.participant_id.clone(),
            side: participant.side.clone(),
            active: observed_pokemon(active),
            bench,
            forced_switch_options,
            shields: participant.shields_remaining,
            switch_cooldown_turns: participant.switch_cooldown_turns,
            move_usage_history: state
                .timeline
                .iter()
                .flat_map(|turn| &turn.events)
                .filter(|event| event.participant_id == participant.participant_id)
                .filter_map(|event| event.move_id.clone())
                .collect(),
        },
        opponent: OpponentObservation {
            participant_id: opponent.participant_id.clone(),
            side: opponent.side.clone(),
            active: OpponentActiveObservation {
                species: opponent_active.species.clone(),
                hp_percentage: hp_percentage(opponent_active.current_hp, opponent_active.max_hp),
                fast_move_in_progress: (opponent_active.move_state.fast_move_turns_remaining > 0)
                    .then(|| opponent_active.move_state.fast_move.clone()),
            },
            bench_count: opponent.team.len().saturating_sub(1),
            shields: opponent.shields_remaining,
            last_used_move,
        },
    })
}

fn observed_pokemon(pokemon: &crate::model::PokemonState) -> ObservedPokemon {
    ObservedPokemon {
        slot: pokemon.slot,
        species: pokemon.species.clone(),
        current_hp: pokemon.current_hp,
        max_hp: pokemon.max_hp,
        hp_percentage: hp_percentage(pokemon.current_hp, pokemon.max_hp),
        energy: pokemon.energy,
        stat_stages: pokemon.stat_stages.clone(),
        fast_move: pokemon.move_state.fast_move.clone(),
        fast_move_turns_remaining: pokemon.move_state.fast_move_turns_remaining,
        charged_moves: pokemon.move_state.charged_moves.clone(),
    }
}

fn hp_percentage(current_hp: u32, max_hp: u32) -> f32 {
    if max_hp == 0 {
        return 0.0;
    }
    current_hp as f32 * 100.0 / max_hp as f32
}

#[cfg(test)]
mod tests {
    use crate::model::{
        BattleSnapshot, CombatStats, ControllerType, Cooldowns, MoveState, ParticipantState,
        PokemonFlags, PokemonState, Side, StatStages, TimelineEvent, TimelineEventKind, TurnRecord,
    };

    use super::{ObservationError, build_observation};

    fn pokemon(slot: usize, species: &str, hp: u32) -> PokemonState {
        PokemonState {
            slot,
            pokemon_id: format!("{species}-{slot}"),
            species: species.to_owned(),
            is_active: slot == 0,
            current_hp: hp,
            max_hp: 100,
            combat_stats: CombatStats {
                attack: 100.0,
                defense: 100.0,
            },
            stat_stages: StatStages::default(),
            energy: 40,
            pending_energy_gain: 0,
            cooldowns: Cooldowns {
                fast_move_ms: 0,
                charged_move_ms: 0,
                swap_ms: 0,
            },
            move_state: MoveState {
                fast_move: "FAST_MOVE".to_owned(),
                fast_move_turns_remaining: 0,
                charged_moves: vec!["CHARGED_MOVE".to_owned()],
                charged_ready: true,
                pending_charge: false,
            },
            flags: PokemonFlags {
                fainted: hp == 0,
                shielded: false,
                shadow: false,
            },
        }
    }

    fn participant(id: &str, side: Side, hp: u32) -> ParticipantState {
        ParticipantState {
            participant_id: id.to_owned(),
            controller_type: ControllerType::Scripted,
            side,
            active_pokemon_index: 0,
            team: vec![pokemon(0, "Active", hp), pokemon(1, "Bench", 60)],
            shields_remaining: 1,
            switch_cooldown_turns: 2,
        }
    }

    fn state() -> BattleSnapshot {
        BattleSnapshot {
            battle_id: "battle-1".to_owned(),
            turn: 8,
            participants: vec![
                participant("left", Side::Left, 75),
                participant("right", Side::Right, 50),
            ],
            pending_charged_moves: Vec::new(),
            pending_forced_swaps: Vec::new(),
            timeline: vec![TurnRecord {
                turn: 7,
                timestamp: 3500,
                events: vec![TimelineEvent {
                    kind: TimelineEventKind::Charged,
                    participant_id: "right".to_owned(),
                    target_participant_id: Some("left".to_owned()),
                    move_id: Some("LAST_CHARGED_MOVE".to_owned()),
                    damage: Some(20),
                    shielded: Some(false),
                    duration_ms: None,
                    species: None,
                }],
            }],
            metadata: Default::default(),
        }
    }

    #[test]
    fn observation_exposes_self_details_but_limits_opponent_details() {
        let mut battle_state = state();
        battle_state.participants[0].team[0]
            .stat_stages
            .apply_attack_delta(1);
        battle_state.participants[0].team[0]
            .move_state
            .fast_move_turns_remaining = 2;
        let observation = build_observation(&battle_state, "left", "2.3", 1).unwrap();

        assert_eq!(observation.self_view.active.current_hp, 75);
        assert_eq!(observation.self_view.active.energy, 40);
        assert_eq!(observation.self_view.active.stat_stages.attack_stage(), 1);
        assert_eq!(observation.self_view.active.fast_move_turns_remaining, 2);
        assert_eq!(observation.opponent.active.species, "Active");
        assert_eq!(observation.opponent.active.hp_percentage, 50.0);
        assert_eq!(observation.opponent.active.fast_move_in_progress, None);
        assert_eq!(observation.opponent.bench_count, 1);
        assert_eq!(
            observation.opponent.last_used_move.as_deref(),
            Some("LAST_CHARGED_MOVE")
        );

        let json = serde_json::to_value(observation).unwrap();
        assert!(json.get("self").is_some());
        assert_eq!(json["self"]["active"]["statStages"]["attack"], 1);
        assert_eq!(json["self"]["active"]["fastMoveTurnsRemaining"], 2);
        assert!(json["opponent"]["active"].get("currentHp").is_none());
        assert!(json["opponent"].get("energy").is_none());
        assert!(json["opponent"]["active"].get("statStages").is_none());
    }

    #[test]
    fn observation_rejects_unknown_participant() {
        let error = build_observation(&state(), "missing", "2.3", 1).unwrap_err();

        assert_eq!(error, ObservationError::ParticipantNotFound);
    }

    #[test]
    fn observation_exposes_forced_switch_candidates_after_faint() {
        let mut battle_state = state();
        battle_state.participants[0].team[0].current_hp = 0;
        battle_state.participants[0].team[0].flags.fainted = true;
        battle_state.participants[0].team[0].is_active = false;
        battle_state
            .pending_forced_swaps
            .push(crate::model::PendingForcedSwap {
                participant_id: "left".to_owned(),
                options: vec![1],
            });

        let observation = build_observation(&battle_state, "left", "2.3", 1).unwrap();

        assert_eq!(observation.self_view.active.current_hp, 0);
        assert_eq!(observation.self_view.forced_switch_options.len(), 1);
        assert_eq!(observation.self_view.forced_switch_options[0].slot, 1);
        assert_eq!(
            observation.self_view.forced_switch_options[0].species,
            "Bench"
        );
    }

    #[test]
    fn opponent_fast_move_visibility_distinguishes_in_progress_from_resolved() {
        let mut battle_state = state();
        let opponent_active = &mut battle_state.participants[1].team[0];
        opponent_active.move_state.fast_move = "VINE_WHIP".to_owned();
        opponent_active.move_state.fast_move_turns_remaining = 1;
        battle_state.timeline[0].events[0].kind = TimelineEventKind::TapInteractionWait;
        battle_state.timeline[0].events[0].move_id = Some("VINE_WHIP".to_owned());
        battle_state.timeline[0].events[0].damage = None;

        let in_progress = build_observation(&battle_state, "left", "2.3", 1).unwrap();

        assert_eq!(
            in_progress.opponent.active.fast_move_in_progress.as_deref(),
            Some("VINE_WHIP")
        );
        assert_eq!(in_progress.opponent.last_used_move, None);

        battle_state.participants[1].team[0]
            .move_state
            .fast_move_turns_remaining = 0;
        battle_state.timeline[0].events[0].kind = TimelineEventKind::TapInteraction;
        battle_state.timeline[0].events[0].damage = Some(7);
        let completed = build_observation(&battle_state, "left", "2.3", 1).unwrap();

        assert_eq!(completed.opponent.active.fast_move_in_progress, None);
        assert_eq!(
            completed.opponent.last_used_move.as_deref(),
            Some("VINE_WHIP")
        );
    }
}
