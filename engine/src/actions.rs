use serde::{Deserialize, Serialize};

use crate::data::DataCatalog;
use crate::model::{ActionRequest, BattleSnapshot, PlayerAction, PokemonState};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackReason {
    MissingAction,
    AmbiguousAction,
    StaleOrFutureTurn,
    InvalidActivePokemon,
    InvalidMove,
    InsufficientEnergy,
    InvalidSwap,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedAction {
    pub participant_id: String,
    pub action: PlayerAction,
    pub fallback_reason: Option<FallbackReason>,
}

pub fn normalize_actions(
    state: &BattleSnapshot,
    requests: &[ActionRequest],
    catalog: &DataCatalog,
) -> Vec<NormalizedAction> {
    state
        .participants
        .iter()
        .map(|participant| {
            let matching: Vec<_> = requests
                .iter()
                .filter(|request| request.participant_id == participant.participant_id)
                .collect();

            let (request, fallback) = match matching.as_slice() {
                [] => (None, Some(FallbackReason::MissingAction)),
                [request] if request.turn != state.turn => {
                    (None, Some(FallbackReason::StaleOrFutureTurn))
                }
                [request] => (Some(*request), None),
                _ => (None, Some(FallbackReason::AmbiguousAction)),
            };

            let Some(active) = participant.team.get(participant.active_pokemon_index) else {
                return normalized_wait(
                    &participant.participant_id,
                    Some(FallbackReason::InvalidActivePokemon),
                );
            };

            if active.flags.fainted || active.current_hp == 0 {
                return normalized_wait(
                    &participant.participant_id,
                    Some(FallbackReason::InvalidActivePokemon),
                );
            }

            let Some(request) = request else {
                return normalized_wait(&participant.participant_id, fallback);
            };

            match &request.action {
                PlayerAction::Wait => normalized_wait(&participant.participant_id, None),
                PlayerAction::FastMove { move_id } if move_id == &active.move_state.fast_move => {
                    NormalizedAction {
                        participant_id: participant.participant_id.clone(),
                        action: request.action.clone(),
                        fallback_reason: None,
                    }
                }
                PlayerAction::FastMove { .. } => normalize_fast_fallback(
                    &participant.participant_id,
                    active,
                    FallbackReason::InvalidMove,
                ),
                PlayerAction::ChargedMove { move_id }
                    if active.move_state.charged_moves.contains(move_id)
                        && catalog
                            .charged_move_cost(move_id)
                            .is_some_and(|cost| active.energy >= cost) =>
                {
                    NormalizedAction {
                        participant_id: participant.participant_id.clone(),
                        action: request.action.clone(),
                        fallback_reason: None,
                    }
                }
                PlayerAction::ChargedMove { move_id }
                    if active.move_state.charged_moves.contains(move_id)
                        && catalog.charged_move_cost(move_id).is_some() =>
                {
                    normalize_fast_fallback(
                        &participant.participant_id,
                        active,
                        FallbackReason::InsufficientEnergy,
                    )
                }
                PlayerAction::ChargedMove { .. } => normalize_fast_fallback(
                    &participant.participant_id,
                    active,
                    FallbackReason::InvalidMove,
                ),
                PlayerAction::Swap { target_slot }
                    if is_legal_swap(
                        participant.switch_cooldown_turns,
                        active,
                        &participant.team,
                        *target_slot,
                    ) =>
                {
                    NormalizedAction {
                        participant_id: participant.participant_id.clone(),
                        action: request.action.clone(),
                        fallback_reason: None,
                    }
                }
                PlayerAction::Swap { .. } => normalized_wait(
                    &participant.participant_id,
                    Some(FallbackReason::InvalidSwap),
                ),
            }
        })
        .collect()
}

fn is_legal_swap(
    switch_cooldown_turns: u32,
    active: &PokemonState,
    team: &[PokemonState],
    target_slot: usize,
) -> bool {
    switch_cooldown_turns == 0
        && team.iter().any(|pokemon| {
            pokemon.slot == target_slot
                && pokemon.slot != active.slot
                && pokemon.current_hp > 0
                && !pokemon.flags.fainted
        })
}

fn normalize_fast_fallback(
    participant_id: &str,
    active: &PokemonState,
    reason: FallbackReason,
) -> NormalizedAction {
    NormalizedAction {
        participant_id: participant_id.to_owned(),
        action: PlayerAction::FastMove {
            move_id: active.move_state.fast_move.clone(),
        },
        fallback_reason: Some(reason),
    }
}

fn normalized_wait(
    participant_id: &str,
    fallback_reason: Option<FallbackReason>,
) -> NormalizedAction {
    NormalizedAction {
        participant_id: participant_id.to_owned(),
        action: PlayerAction::Wait,
        fallback_reason,
    }
}

#[cfg(test)]
mod tests {
    use crate::data::DataCatalog;
    use crate::model::{
        BattleSnapshot, CombatStats, ControllerType, Cooldowns, MoveState, ParticipantState,
        PokemonFlags, PokemonState, Side, StatStages,
    };

    use super::{FallbackReason, normalize_actions};

    fn pokemon(slot: usize, hp: u32, fast_move: &str, charged_moves: &[&str]) -> PokemonState {
        PokemonState {
            slot,
            pokemon_id: format!("pokemon-{slot}"),
            species: format!("Species {slot}"),
            is_active: slot == 0,
            current_hp: hp,
            max_hp: 100,
            combat_stats: CombatStats {
                attack: 100.0,
                defense: 100.0,
            },
            stat_stages: StatStages::default(),
            energy: 20,
            pending_energy_gain: 0,
            cooldowns: Cooldowns {
                fast_move_ms: 0,
                charged_move_ms: 0,
                swap_ms: 0,
            },
            move_state: MoveState {
                fast_move: fast_move.to_owned(),
                charged_moves: charged_moves.iter().map(|id| (*id).to_owned()).collect(),
                charged_ready: false,
                pending_charge: false,
            },
            flags: PokemonFlags {
                fainted: hp == 0,
                shielded: false,
                shadow: false,
            },
        }
    }

    fn state() -> BattleSnapshot {
        BattleSnapshot {
            battle_id: "battle-1".to_owned(),
            turn: 4,
            participants: vec![ParticipantState {
                participant_id: "left".to_owned(),
                controller_type: ControllerType::Scripted,
                side: Side::Left,
                active_pokemon_index: 0,
                team: vec![
                    pokemon(0, 100, "fast-1", &["POWER_WHIP"]),
                    pokemon(1, 100, "fast-2", &[]),
                ],
                shields_remaining: 2,
                switch_cooldown_turns: 0,
            }],
            pending_forced_swaps: Vec::new(),
            timeline: Vec::new(),
            metadata: Default::default(),
        }
    }

    #[test]
    fn underfunded_charged_move_falls_back_to_fast_move() {
        let state = state();
        let requests = [crate::model::ActionRequest {
            participant_id: "left".to_owned(),
            turn: 4,
            action: crate::model::PlayerAction::ChargedMove {
                move_id: "POWER_WHIP".to_owned(),
            },
        }];
        let catalog = DataCatalog::embedded().unwrap();

        let normalized = normalize_actions(&state, &requests, &catalog);

        assert_eq!(
            normalized[0].action,
            crate::model::PlayerAction::FastMove {
                move_id: "fast-1".to_owned()
            }
        );
        assert_eq!(
            normalized[0].fallback_reason,
            Some(FallbackReason::InsufficientEnergy)
        );
    }

    #[test]
    fn missing_action_defaults_to_wait() {
        let catalog = DataCatalog::embedded().unwrap();
        let normalized = normalize_actions(&state(), &[], &catalog);

        assert_eq!(normalized[0].action, crate::model::PlayerAction::Wait);
        assert_eq!(
            normalized[0].fallback_reason,
            Some(FallbackReason::MissingAction)
        );
    }

    #[test]
    fn valid_swap_requires_available_living_bench_pokemon() {
        let state = state();
        let requests = [crate::model::ActionRequest {
            participant_id: "left".to_owned(),
            turn: 4,
            action: crate::model::PlayerAction::Swap { target_slot: 1 },
        }];

        let catalog = DataCatalog::embedded().unwrap();
        let normalized = normalize_actions(&state, &requests, &catalog);

        assert_eq!(normalized[0].action, requests[0].action);
        assert_eq!(normalized[0].fallback_reason, None);
    }

    #[test]
    fn switching_out_resets_stages_but_preserves_hp_and_energy() {
        let mut pokemon = pokemon(0, 80, "fast-1", &[]);
        pokemon.energy = 60;
        pokemon.stat_stages.apply_attack_delta(2);
        pokemon.stat_stages.apply_defense_delta(-1);

        pokemon.reset_stat_stages_on_switch_out();

        assert_eq!(pokemon.stat_stages.attack_stage(), 0);
        assert_eq!(pokemon.stat_stages.defense_stage(), 0);
        assert_eq!(pokemon.current_hp, 80);
        assert_eq!(pokemon.energy, 60);
    }
}
