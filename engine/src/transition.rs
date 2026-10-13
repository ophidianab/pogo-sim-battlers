use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::actions::normalize_actions;
use crate::damage::{DamageCombatant, calculate_damage};
use crate::data::DataCatalog;
use crate::model::{
    ActionRequest, BattleSnapshot, PendingChargedMove, PendingForcedSwap, PlayerAction,
    ShieldDecision, ShieldDecisionRequest, TimelineEvent, TimelineEventKind, TurnRecord,
};

#[derive(Clone, Debug, PartialEq)]
pub struct TurnTransition {
    pub next_snapshot: BattleSnapshot,
    pub timeline_entries: Vec<TimelineEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransitionError {
    ParticipantCount,
    InvalidActivePokemon(String),
    UnknownSpecies(String),
    UnknownMove(String),
    UnsupportedAction(String),
    Damage(String),
}

impl Display for TransitionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParticipantCount => formatter.write_str("turn resolution requires 1v1"),
            Self::InvalidActivePokemon(participant_id) => {
                write!(
                    formatter,
                    "participant {participant_id} has no active Pokemon"
                )
            }
            Self::UnknownSpecies(species) => {
                write!(formatter, "unknown Pokemon species: {species}")
            }
            Self::UnknownMove(move_id) => write!(formatter, "unknown move: {move_id}"),
            Self::UnsupportedAction(participant_id) => write!(
                formatter,
                "turn resolution does not yet support this action for {participant_id}"
            ),
            Self::Damage(error) => formatter.write_str(error),
        }
    }
}

impl Error for TransitionError {}

struct PendingFastMove {
    attacker_index: usize,
    target_index: usize,
    damage: u32,
    energy_gain: u32,
    event: TimelineEvent,
}

pub fn resolve_turn(
    snapshot: &BattleSnapshot,
    requests: &[ActionRequest],
    catalog: &DataCatalog,
) -> Result<TurnTransition, TransitionError> {
    resolve_turn_with_shield_decisions(snapshot, requests, &[], catalog)
}

pub fn resolve_turn_with_shield_decisions(
    snapshot: &BattleSnapshot,
    requests: &[ActionRequest],
    shield_decisions: &[ShieldDecisionRequest],
    catalog: &DataCatalog,
) -> Result<TurnTransition, TransitionError> {
    if snapshot.participants.len() != 2 {
        return Err(TransitionError::ParticipantCount);
    }
    if !snapshot.pending_forced_swaps.is_empty() {
        return resolve_pending_forced_swaps(snapshot, requests);
    }

    let actionable_requests: Vec<_> = requests
        .iter()
        .filter(|request| {
            let charged_locked = snapshot
                .pending_charged_moves
                .iter()
                .any(|pending| pending.participant_id == request.participant_id);
            let fast_locked = snapshot
                .participants
                .iter()
                .find(|participant| participant.participant_id == request.participant_id)
                .and_then(|participant| participant.team.get(participant.active_pokemon_index))
                .is_some_and(|pokemon| pokemon.move_state.fast_move_turns_remaining > 0);
            !charged_locked && !fast_locked
        })
        .cloned()
        .collect();
    let normalized = normalize_actions(snapshot, &actionable_requests, catalog);
    let mut next_snapshot = snapshot.clone();
    let mut timeline_entries = Vec::new();

    for (participant_index, action) in normalized.iter().enumerate() {
        let PlayerAction::Swap { target_slot } = action.action else {
            continue;
        };
        let incoming_index = next_snapshot.participants[participant_index]
            .team
            .iter()
            .position(|pokemon| pokemon.slot == target_slot)
            .ok_or_else(|| TransitionError::UnsupportedAction(action.participant_id.clone()))?;
        let (participant_id, species) =
            switch_active_to(&mut next_snapshot, participant_index, incoming_index);
        timeline_entries.push(TimelineEvent {
            kind: TimelineEventKind::Swap,
            participant_id,
            target_participant_id: None,
            move_id: None,
            damage: None,
            shielded: None,
            duration_ms: None,
            species: Some(species),
        });
    }

    let mut fainted_participants = Vec::new();
    let mut complete_fast_after_charge = vec![false; next_snapshot.participants.len()];
    for pending_charge in &snapshot.pending_charged_moves {
        let Some(attacker_index) = next_snapshot
            .participants
            .iter()
            .position(|participant| participant.participant_id == pending_charge.participant_id)
        else {
            continue;
        };
        let target_index = 1 - attacker_index;
        if active_pokemon(&next_snapshot, attacker_index).is_ok()
            && active_pokemon(&next_snapshot, target_index).is_ok()
            && next_snapshot.participants[target_index].team
                [next_snapshot.participants[target_index].active_pokemon_index]
                .move_state
                .fast_move_turns_remaining
                > 0
        {
            complete_fast_after_charge[target_index] = true;
        }
    }

    let mut charged_order = snapshot.pending_charged_moves.clone();
    charged_order.sort_by(|left, right| {
        let left_index = snapshot
            .participants
            .iter()
            .position(|participant| participant.participant_id == left.participant_id)
            .unwrap_or(usize::MAX);
        let right_index = snapshot
            .participants
            .iter()
            .position(|participant| participant.participant_id == right.participant_id)
            .unwrap_or(usize::MAX);
        let left_attack = snapshot
            .participants
            .get(left_index)
            .and_then(|participant| participant.team.get(participant.active_pokemon_index))
            .map(|pokemon| pokemon.combat_stats.attack)
            .unwrap_or_default();
        let right_attack = snapshot
            .participants
            .get(right_index)
            .and_then(|participant| participant.team.get(participant.active_pokemon_index))
            .map(|pokemon| pokemon.combat_stats.attack)
            .unwrap_or_default();
        right_attack
            .total_cmp(&left_attack)
            .then_with(|| left_index.cmp(&right_index))
    });
    next_snapshot.pending_charged_moves.clear();

    for pending_charge in charged_order {
        let Some(attacker_index) = next_snapshot
            .participants
            .iter()
            .position(|participant| participant.participant_id == pending_charge.participant_id)
        else {
            continue;
        };
        let target_index = 1 - attacker_index;
        if active_pokemon(&next_snapshot, attacker_index).is_err() {
            let participant = &mut next_snapshot.participants[attacker_index];
            if let Some(active) = participant.team.get_mut(participant.active_pokemon_index) {
                active.move_state.pending_charge = false;
            }
            continue;
        }
        active_pokemon_mut(&mut next_snapshot, attacker_index)?
            .move_state
            .pending_charge = false;
        if active_pokemon(&next_snapshot, target_index).is_err() {
            continue;
        }
        let move_id = &pending_charge.move_id;

        let move_definition = catalog
            .move_definition(move_id)
            .ok_or_else(|| TransitionError::UnknownMove(move_id.clone()))?;
        let damage = calculate_active_move_damage(
            &next_snapshot,
            catalog,
            attacker_index,
            target_index,
            move_id,
        )?;
        let target_participant_id = next_snapshot.participants[target_index]
            .participant_id
            .clone();
        let matching_decisions: Vec<_> = shield_decisions
            .iter()
            .filter(|decision| {
                decision.participant_id == target_participant_id && decision.turn == snapshot.turn
            })
            .collect();
        let shielded = matches!(
            matching_decisions.as_slice(),
            [decision] if decision.decision == ShieldDecision::Shield
        ) && next_snapshot.participants[target_index].shields_remaining > 0;

        if shielded {
            next_snapshot.participants[target_index].shields_remaining -= 1;
        }
        let applied_damage = if shielded { 1 } else { damage };
        timeline_entries.push(TimelineEvent {
            kind: TimelineEventKind::Charged,
            participant_id: next_snapshot.participants[attacker_index]
                .participant_id
                .clone(),
            target_participant_id: Some(target_participant_id),
            move_id: Some(move_id.clone()),
            damage: Some(applied_damage),
            shielded: Some(shielded),
            duration_ms: Some(move_definition.cooldown),
            species: None,
        });

        let target = active_pokemon_mut(&mut next_snapshot, target_index)?;
        let fainted = target.current_hp > 0 && target.current_hp <= applied_damage;
        target.current_hp = target.current_hp.saturating_sub(applied_damage);
        if fainted {
            target.flags.fainted = true;
            fainted_participants.push(target_index);
            target.is_active = false;
            target.reset_stat_stages_on_switch_out();
            target.cancel_fast_move();
        }
    }

    for (participant_index, action) in normalized.iter().enumerate() {
        let PlayerAction::ChargedMove { move_id } = &action.action else {
            continue;
        };
        if active_pokemon(&next_snapshot, participant_index).is_err()
            || active_pokemon(&next_snapshot, 1 - participant_index).is_err()
        {
            continue;
        }
        let energy_cost = catalog
            .charged_move_cost(move_id)
            .ok_or_else(|| TransitionError::UnknownMove(move_id.clone()))?;
        let attacker = active_pokemon_mut(&mut next_snapshot, participant_index)?;
        attacker.energy = attacker.energy.saturating_sub(energy_cost);
        attacker.move_state.pending_charge = true;
        next_snapshot
            .pending_charged_moves
            .push(PendingChargedMove {
                participant_id: action.participant_id.clone(),
                move_id: move_id.clone(),
            });
    }

    for (attacker_index, action) in normalized.iter().enumerate() {
        let PlayerAction::FastMove { move_id } = &action.action else {
            if matches!(
                action.action,
                PlayerAction::Wait | PlayerAction::Swap { .. } | PlayerAction::ChargedMove { .. }
            ) {
                continue;
            }
            return Err(TransitionError::UnsupportedAction(
                action.participant_id.clone(),
            ));
        };
        let target_index = 1 - attacker_index;
        if active_pokemon(&next_snapshot, attacker_index).is_err()
            || active_pokemon(&next_snapshot, target_index).is_err()
        {
            continue;
        }
        let move_definition = catalog
            .move_definition(move_id)
            .ok_or_else(|| TransitionError::UnknownMove(move_id.clone()))?;
        let attacker = active_pokemon_mut(&mut next_snapshot, attacker_index)?;
        if attacker.move_state.fast_move_turns_remaining == 0 {
            attacker.move_state.fast_move_turns_remaining = move_definition.turns.max(1);
            attacker.pending_energy_gain = move_definition.energy_gain;
            attacker.cooldowns.fast_move_ms = move_definition.cooldown;
        }
    }

    let mut pending_moves = Vec::new();
    for attacker_index in 0..next_snapshot.participants.len() {
        if active_pokemon(&next_snapshot, attacker_index).is_err() {
            continue;
        }
        let target_index = 1 - attacker_index;
        let remaining = next_snapshot.participants[attacker_index].team
            [next_snapshot.participants[attacker_index].active_pokemon_index]
            .move_state
            .fast_move_turns_remaining;
        if remaining == 0 {
            continue;
        }

        let attacker = active_pokemon_mut(&mut next_snapshot, attacker_index)?;
        if complete_fast_after_charge[attacker_index] {
            attacker.move_state.fast_move_turns_remaining = 1;
        }
        attacker.move_state.fast_move_turns_remaining = attacker
            .move_state
            .fast_move_turns_remaining
            .saturating_sub(1);
        attacker.cooldowns.fast_move_ms = attacker.cooldowns.fast_move_ms.saturating_sub(500);
        let move_id = attacker.move_state.fast_move.clone();
        let completed = attacker.move_state.fast_move_turns_remaining == 0;
        let energy_gain = if completed {
            let energy_gain = attacker.pending_energy_gain;
            attacker.pending_energy_gain = 0;
            attacker.cooldowns.fast_move_ms = 0;
            energy_gain
        } else {
            0
        };

        if active_pokemon(&next_snapshot, target_index).is_err() {
            active_pokemon_mut(&mut next_snapshot, attacker_index)?.cancel_fast_move();
            continue;
        }

        if completed {
            let move_definition = catalog
                .move_definition(&move_id)
                .ok_or_else(|| TransitionError::UnknownMove(move_id.clone()))?;
            let damage = calculate_active_move_damage(
                &next_snapshot,
                catalog,
                attacker_index,
                target_index,
                &move_id,
            )?;
            pending_moves.push(PendingFastMove {
                attacker_index,
                target_index,
                damage,
                energy_gain,
                event: TimelineEvent {
                    kind: TimelineEventKind::TapInteraction,
                    participant_id: next_snapshot.participants[attacker_index]
                        .participant_id
                        .clone(),
                    target_participant_id: Some(
                        next_snapshot.participants[target_index]
                            .participant_id
                            .clone(),
                    ),
                    move_id: Some(move_id),
                    damage: Some(damage),
                    shielded: None,
                    duration_ms: Some(move_definition.cooldown),
                    species: None,
                },
            });
        } else {
            timeline_entries.push(TimelineEvent {
                kind: TimelineEventKind::TapInteractionWait,
                participant_id: next_snapshot.participants[attacker_index]
                    .participant_id
                    .clone(),
                target_participant_id: Some(
                    next_snapshot.participants[target_index]
                        .participant_id
                        .clone(),
                ),
                move_id: Some(move_id),
                damage: None,
                shielded: None,
                duration_ms: Some(500),
                species: None,
            });
        }
    }

    for pending in pending_moves {
        let attacker = active_pokemon_mut(&mut next_snapshot, pending.attacker_index)?;
        attacker.energy = attacker.energy.saturating_add(pending.energy_gain);
        timeline_entries.push(pending.event);

        let target_index = pending.target_index;
        let damage = pending.damage;
        let target = active_pokemon_mut(&mut next_snapshot, target_index)?;
        let fainted = target.current_hp > 0 && target.current_hp <= damage;
        target.current_hp = target.current_hp.saturating_sub(damage);
        if fainted {
            target.flags.fainted = true;
            target.is_active = false;
            target.reset_stat_stages_on_switch_out();
            fainted_participants.push(target_index);
        }
    }

    let cancelled_charges: Vec<_> = next_snapshot
        .pending_charged_moves
        .iter()
        .filter(|pending| {
            !next_snapshot
                .participants
                .iter()
                .find(|participant| participant.participant_id == pending.participant_id)
                .and_then(|participant| participant.team.get(participant.active_pokemon_index))
                .is_some_and(|pokemon| pokemon.current_hp > 0 && !pokemon.flags.fainted)
        })
        .map(|pending| pending.participant_id.clone())
        .collect();
    for participant_id in &cancelled_charges {
        if let Some(participant) = next_snapshot
            .participants
            .iter_mut()
            .find(|participant| &participant.participant_id == participant_id)
            && let Some(active) = participant.team.get_mut(participant.active_pokemon_index)
        {
            active.move_state.pending_charge = false;
        }
    }
    next_snapshot
        .pending_charged_moves
        .retain(|pending| !cancelled_charges.contains(&pending.participant_id));

    for participant_index in fainted_participants {
        let outgoing_index = next_snapshot.participants[participant_index].active_pokemon_index;
        let participant_id = next_snapshot.participants[participant_index]
            .participant_id
            .clone();
        let fainted_species = next_snapshot.participants[participant_index].team[outgoing_index]
            .species
            .clone();
        next_snapshot.participants[participant_index].team[outgoing_index].is_active = false;
        next_snapshot.participants[participant_index].team[outgoing_index]
            .reset_stat_stages_on_switch_out();
        timeline_entries.push(TimelineEvent {
            kind: TimelineEventKind::Faint,
            participant_id: participant_id.clone(),
            target_participant_id: None,
            move_id: None,
            damage: None,
            shielded: None,
            duration_ms: None,
            species: Some(fainted_species),
        });

        let replacements: Vec<_> = next_snapshot.participants[participant_index]
            .team
            .iter()
            .enumerate()
            .filter(|(index, pokemon)| {
                *index != outgoing_index && pokemon.current_hp > 0 && !pokemon.flags.fainted
            })
            .map(|(index, pokemon)| (index, pokemon.slot))
            .collect();
        if replacements.len() == 1 {
            let replacement_index = replacements[0].0;
            let (participant_id, species) =
                switch_active_to(&mut next_snapshot, participant_index, replacement_index);
            timeline_entries.push(TimelineEvent {
                kind: TimelineEventKind::Swap,
                participant_id,
                target_participant_id: None,
                move_id: None,
                damage: None,
                shielded: None,
                duration_ms: None,
                species: Some(species),
            });
        } else if replacements.len() > 1 {
            next_snapshot.pending_forced_swaps.push(PendingForcedSwap {
                participant_id,
                options: replacements.iter().map(|(_, slot)| *slot).collect(),
            });
        }
    }

    let turn_record = TurnRecord {
        turn: snapshot.turn,
        timestamp: u64::from(snapshot.turn) * 500,
        events: timeline_entries.clone(),
    };
    next_snapshot.turn = next_snapshot.turn.saturating_add(1);
    next_snapshot.timeline.push(turn_record);

    Ok(TurnTransition {
        next_snapshot,
        timeline_entries,
    })
}

fn calculate_active_move_damage(
    snapshot: &BattleSnapshot,
    catalog: &DataCatalog,
    attacker_index: usize,
    target_index: usize,
    move_id: &str,
) -> Result<u32, TransitionError> {
    let attacker = active_pokemon(snapshot, attacker_index)?;
    let target = active_pokemon(snapshot, target_index)?;
    let move_definition = catalog
        .move_definition(move_id)
        .ok_or_else(|| TransitionError::UnknownMove(move_id.to_owned()))?;
    let attacker_definition = catalog
        .pokemon_definition(&attacker.species)
        .ok_or_else(|| TransitionError::UnknownSpecies(attacker.species.clone()))?;
    let target_definition = catalog
        .pokemon_definition(&target.species)
        .ok_or_else(|| TransitionError::UnknownSpecies(target.species.clone()))?;
    calculate_damage(
        catalog,
        DamageCombatant {
            combat_stats: &attacker.combat_stats,
            stat_stages: &attacker.stat_stages,
            shadow: attacker.flags.shadow,
            types: &attacker_definition.types,
        },
        DamageCombatant {
            combat_stats: &target.combat_stats,
            stat_stages: &target.stat_stages,
            shadow: target.flags.shadow,
            types: &target_definition.types,
        },
        move_definition,
        1.0,
    )
    .map_err(|error| TransitionError::Damage(error.to_string()))
}

fn resolve_pending_forced_swaps(
    snapshot: &BattleSnapshot,
    requests: &[ActionRequest],
) -> Result<TurnTransition, TransitionError> {
    let mut next_snapshot = snapshot.clone();
    let mut pending_choices = Vec::new();
    let mut timeline_entries = Vec::new();

    for pending in &snapshot.pending_forced_swaps {
        let matching: Vec<_> = requests
            .iter()
            .filter(|request| request.participant_id == pending.participant_id)
            .collect();
        let [request] = matching.as_slice() else {
            pending_choices.push(pending.clone());
            continue;
        };
        if request.turn != snapshot.turn {
            pending_choices.push(pending.clone());
            continue;
        }
        let PlayerAction::Swap { target_slot } = &request.action else {
            pending_choices.push(pending.clone());
            continue;
        };
        if !pending.options.contains(target_slot) {
            pending_choices.push(pending.clone());
            continue;
        }

        let Some(participant_index) = next_snapshot
            .participants
            .iter()
            .position(|participant| participant.participant_id == pending.participant_id)
        else {
            pending_choices.push(pending.clone());
            continue;
        };
        let Some(incoming_index) = next_snapshot.participants[participant_index]
            .team
            .iter()
            .position(|pokemon| {
                pokemon.slot == *target_slot && pokemon.current_hp > 0 && !pokemon.flags.fainted
            })
        else {
            pending_choices.push(pending.clone());
            continue;
        };
        let (participant_id, species) =
            switch_active_to(&mut next_snapshot, participant_index, incoming_index);
        timeline_entries.push(TimelineEvent {
            kind: TimelineEventKind::Swap,
            participant_id,
            target_participant_id: None,
            move_id: None,
            damage: None,
            shielded: None,
            duration_ms: None,
            species: Some(species),
        });
    }

    next_snapshot.pending_forced_swaps = pending_choices;
    if !timeline_entries.is_empty() {
        if let Some(turn_record) = next_snapshot.timeline.last_mut() {
            turn_record.events.extend(timeline_entries.iter().cloned());
        } else {
            let turn = snapshot.turn.saturating_sub(1);
            next_snapshot.timeline.push(TurnRecord {
                turn,
                timestamp: u64::from(turn) * 500,
                events: timeline_entries.clone(),
            });
        }
    }

    Ok(TurnTransition {
        next_snapshot,
        timeline_entries,
    })
}

fn active_pokemon(
    snapshot: &BattleSnapshot,
    participant_index: usize,
) -> Result<&crate::model::PokemonState, TransitionError> {
    let participant = &snapshot.participants[participant_index];
    participant
        .team
        .get(participant.active_pokemon_index)
        .filter(|pokemon| !pokemon.flags.fainted && pokemon.current_hp > 0)
        .ok_or_else(|| TransitionError::InvalidActivePokemon(participant.participant_id.clone()))
}

fn active_pokemon_mut(
    snapshot: &mut BattleSnapshot,
    participant_index: usize,
) -> Result<&mut crate::model::PokemonState, TransitionError> {
    let participant = &mut snapshot.participants[participant_index];
    participant
        .team
        .get_mut(participant.active_pokemon_index)
        .ok_or_else(|| TransitionError::InvalidActivePokemon(participant.participant_id.clone()))
}

fn switch_active_to(
    snapshot: &mut BattleSnapshot,
    participant_index: usize,
    incoming_index: usize,
) -> (String, String) {
    let participant = &mut snapshot.participants[participant_index];
    let outgoing_index = participant.active_pokemon_index;
    let participant_id = participant.participant_id.clone();
    let incoming_species = participant.team[incoming_index].species.clone();

    participant.team[outgoing_index].reset_stat_stages_on_switch_out();
    participant.team[outgoing_index].cancel_fast_move();
    participant.team[outgoing_index].is_active = false;
    participant.team[incoming_index].is_active = true;
    participant.active_pokemon_index = incoming_index;

    (participant_id, incoming_species)
}

#[cfg(test)]
mod tests {
    use crate::actions::{FallbackReason, normalize_actions};
    use crate::data::DataCatalog;
    use crate::model::{
        ActionRequest, BattleSnapshot, CombatStats, ControllerType, Cooldowns, MoveState,
        ParticipantState, PendingChargedMove, PlayerAction, PokemonFlags, PokemonState,
        ShieldDecision, ShieldDecisionRequest, Side, StatStages, TimelineEventKind,
    };

    use super::{resolve_turn, resolve_turn_with_shield_decisions};

    fn pokemon(slot: usize, species: &str) -> PokemonState {
        PokemonState {
            slot,
            pokemon_id: format!("{species}-{slot}"),
            species: species.to_owned(),
            is_active: true,
            current_hp: 100,
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
                fast_move: "TACKLE".to_owned(),
                fast_move_turns_remaining: 0,
                charged_moves: Vec::new(),
                charged_ready: false,
                pending_charge: false,
            },
            flags: PokemonFlags {
                fainted: false,
                shielded: false,
                shadow: false,
            },
        }
    }

    fn participant(participant_id: &str, side: Side, species: &str) -> ParticipantState {
        ParticipantState {
            participant_id: participant_id.to_owned(),
            controller_type: ControllerType::Scripted,
            side,
            active_pokemon_index: 0,
            team: vec![pokemon(0, species)],
            shields_remaining: 2,
            switch_cooldown_turns: 0,
        }
    }

    fn snapshot() -> BattleSnapshot {
        BattleSnapshot {
            battle_id: "fixed-1v1".to_owned(),
            turn: 4,
            participants: vec![
                participant("left", Side::Left, "bulbasaur"),
                participant("right", Side::Right, "squirtle"),
            ],
            pending_charged_moves: Vec::new(),
            pending_forced_swaps: Vec::new(),
            timeline: Vec::new(),
            metadata: Default::default(),
        }
    }

    #[test]
    fn both_fast_moves_resolve_simultaneously_and_record_the_turn() {
        let state = snapshot();
        let requests = [
            ActionRequest {
                participant_id: "left".to_owned(),
                turn: 4,
                action: PlayerAction::FastMove {
                    move_id: "TACKLE".to_owned(),
                },
            },
            ActionRequest {
                participant_id: "right".to_owned(),
                turn: 4,
                action: PlayerAction::FastMove {
                    move_id: "TACKLE".to_owned(),
                },
            },
        ];
        let catalog = DataCatalog::embedded().unwrap();

        let transition = resolve_turn(&state, &requests, &catalog).unwrap();
        let next = transition.next_snapshot;

        assert_eq!(next.turn, 5);
        assert_eq!(next.participants[0].team[0].current_hp, 98);
        assert_eq!(next.participants[1].team[0].current_hp, 98);
        assert_eq!(next.participants[0].team[0].energy, 23);
        assert_eq!(next.participants[1].team[0].energy, 23);
        assert_eq!(next.timeline.len(), 1);
        assert_eq!(next.timeline[0].turn, 4);
        assert_eq!(next.timeline[0].timestamp, 2_000);
        assert_eq!(next.timeline[0].events.len(), 2);
        assert!(
            next.timeline[0]
                .events
                .iter()
                .all(|event| event.kind == TimelineEventKind::TapInteraction)
        );
        assert_eq!(next.timeline[0].events[0].damage, Some(2));
        assert_eq!(next.timeline[0].events[1].damage, Some(2));
        assert_eq!(transition.timeline_entries, next.timeline[0].events);
        assert_eq!(state.participants[0].team[0].current_hp, 100);
        assert_eq!(state.participants[0].team[0].energy, 20);
    }

    #[test]
    fn two_turn_fast_move_locks_action_and_delays_damage_and_energy() {
        let mut state = snapshot();
        let catalog = DataCatalog::embedded().unwrap();
        let charged_cost = catalog.charged_move_cost("POWER_WHIP").unwrap();
        let attacker = &mut state.participants[0].team[0];
        attacker.energy = charged_cost;
        attacker.move_state.fast_move = "VINE_WHIP".to_owned();
        attacker
            .move_state
            .charged_moves
            .push("POWER_WHIP".to_owned());
        let first_turn_requests = [
            ActionRequest {
                participant_id: "left".to_owned(),
                turn: 4,
                action: PlayerAction::FastMove {
                    move_id: "VINE_WHIP".to_owned(),
                },
            },
            ActionRequest {
                participant_id: "right".to_owned(),
                turn: 4,
                action: PlayerAction::Wait,
            },
        ];

        let first_turn = resolve_turn(&state, &first_turn_requests, &catalog).unwrap();
        let in_progress = first_turn.next_snapshot;

        assert_eq!(in_progress.turn, 5);
        assert_eq!(in_progress.participants[1].team[0].current_hp, 100);
        assert_eq!(in_progress.participants[0].team[0].energy, charged_cost);
        assert_eq!(in_progress.participants[0].team[0].pending_energy_gain, 8);
        assert_eq!(
            in_progress.participants[0].team[0]
                .move_state
                .fast_move_turns_remaining,
            1
        );
        assert_eq!(
            in_progress.participants[0].team[0].cooldowns.fast_move_ms,
            500
        );
        assert_eq!(
            in_progress.timeline[0].events[0].kind,
            TimelineEventKind::TapInteractionWait
        );

        let locked_turn_requests = [
            ActionRequest {
                participant_id: "left".to_owned(),
                turn: 5,
                action: PlayerAction::ChargedMove {
                    move_id: "POWER_WHIP".to_owned(),
                },
            },
            ActionRequest {
                participant_id: "right".to_owned(),
                turn: 5,
                action: PlayerAction::Wait,
            },
        ];
        let second_turn = resolve_turn(&in_progress, &locked_turn_requests, &catalog).unwrap();
        let completed = second_turn.next_snapshot;

        assert_eq!(completed.turn, 6);
        assert_eq!(completed.participants[1].team[0].current_hp, 93);
        assert_eq!(completed.participants[0].team[0].energy, charged_cost + 8);
        assert_eq!(completed.participants[0].team[0].pending_energy_gain, 0);
        assert_eq!(
            completed.participants[0].team[0]
                .move_state
                .fast_move_turns_remaining,
            0
        );
        assert_eq!(completed.participants[0].team[0].cooldowns.fast_move_ms, 0);
        assert!(completed.pending_charged_moves.is_empty());
        assert_eq!(
            completed.timeline[1].events[0].kind,
            TimelineEventKind::TapInteraction
        );
        assert_eq!(
            completed.timeline[1].events[0].move_id.as_deref(),
            Some("VINE_WHIP")
        );
        assert_eq!(state.participants[1].team[0].current_hp, 100);
    }

    #[test]
    fn queued_charged_move_forces_surviving_fast_move_to_complete_first() {
        let mut state = snapshot();
        state.turn = 5;
        let catalog = DataCatalog::embedded().unwrap();
        let volt_switch = catalog.move_definition("VOLT_SWITCH").unwrap();
        let left = &mut state.participants[0].team[0];
        left.move_state.fast_move = "VOLT_SWITCH".to_owned();
        left.move_state.fast_move_turns_remaining = 2;
        left.pending_energy_gain = volt_switch.energy_gain;
        left.cooldowns.fast_move_ms = volt_switch.cooldown;
        let right = &mut state.participants[1].team[0];
        right.move_state.pending_charge = true;
        state.pending_charged_moves.push(PendingChargedMove {
            participant_id: "right".to_owned(),
            move_id: "POWER_WHIP".to_owned(),
        });
        let requests = [
            ActionRequest {
                participant_id: "left".to_owned(),
                turn: 5,
                action: PlayerAction::Wait,
            },
            ActionRequest {
                participant_id: "right".to_owned(),
                turn: 5,
                action: PlayerAction::Wait,
            },
        ];

        let transition = resolve_turn(&state, &requests, &catalog).unwrap();
        let next = transition.next_snapshot;

        assert_eq!(next.participants[1].team[0].current_hp, 85);
        assert_eq!(
            next.participants[0].team[0].energy,
            20 + volt_switch.energy_gain
        );
        assert_eq!(
            next.participants[0].team[0]
                .move_state
                .fast_move_turns_remaining,
            0
        );
        assert_eq!(next.timeline[0].events.len(), 2);
        assert_eq!(next.timeline[0].events[0].kind, TimelineEventKind::Charged);
        assert_eq!(
            next.timeline[0].events[1].kind,
            TimelineEventKind::TapInteraction
        );
        assert_eq!(
            next.timeline[0].events[1].move_id.as_deref(),
            Some("VOLT_SWITCH")
        );
    }

    #[test]
    fn charged_move_faint_cancels_in_progress_fast_move_and_energy_gain() {
        let mut state = snapshot();
        state.turn = 5;
        let catalog = DataCatalog::embedded().unwrap();
        let volt_switch = catalog.move_definition("VOLT_SWITCH").unwrap();
        let left = &mut state.participants[0].team[0];
        left.current_hp = 1;
        left.move_state.fast_move = "VOLT_SWITCH".to_owned();
        left.move_state.fast_move_turns_remaining = 2;
        left.pending_energy_gain = volt_switch.energy_gain;
        left.cooldowns.fast_move_ms = volt_switch.cooldown;
        let right = &mut state.participants[1].team[0];
        right.combat_stats.attack = 200.0;
        right.move_state.pending_charge = true;
        state.pending_charged_moves.push(PendingChargedMove {
            participant_id: "right".to_owned(),
            move_id: "POWER_WHIP".to_owned(),
        });
        let requests = [
            ActionRequest {
                participant_id: "left".to_owned(),
                turn: 5,
                action: PlayerAction::Wait,
            },
            ActionRequest {
                participant_id: "right".to_owned(),
                turn: 5,
                action: PlayerAction::Wait,
            },
        ];

        let transition = resolve_turn(&state, &requests, &catalog).unwrap();
        let next = transition.next_snapshot;

        assert!(next.participants[0].team[0].flags.fainted);
        assert_eq!(next.participants[0].team[0].energy, 20);
        assert_eq!(next.participants[0].team[0].pending_energy_gain, 0);
        assert_eq!(
            next.participants[0].team[0]
                .move_state
                .fast_move_turns_remaining,
            0
        );
        assert_eq!(next.timeline[0].events.len(), 2);
        assert_eq!(next.timeline[0].events[0].kind, TimelineEventKind::Charged);
        assert_eq!(next.timeline[0].events[1].kind, TimelineEventKind::Faint);
    }

    #[test]
    fn fast_move_energy_is_spendable_only_on_the_following_turn() {
        let mut state = snapshot();
        let catalog = DataCatalog::embedded().unwrap();
        let charged_cost = catalog.charged_move_cost("POWER_WHIP").unwrap();
        let active = &mut state.participants[0].team[0];
        active.energy = charged_cost - 1;
        active
            .move_state
            .charged_moves
            .push("POWER_WHIP".to_owned());

        let same_turn_requests = [
            ActionRequest {
                participant_id: "left".to_owned(),
                turn: state.turn,
                action: PlayerAction::ChargedMove {
                    move_id: "POWER_WHIP".to_owned(),
                },
            },
            ActionRequest {
                participant_id: "right".to_owned(),
                turn: state.turn,
                action: PlayerAction::Wait,
            },
        ];
        let normalized = normalize_actions(&state, &same_turn_requests, &catalog);
        assert_eq!(
            normalized[0].action,
            PlayerAction::FastMove {
                move_id: "TACKLE".to_owned()
            }
        );
        assert_eq!(
            normalized[0].fallback_reason,
            Some(FallbackReason::InsufficientEnergy)
        );

        let transition = resolve_turn(&state, &same_turn_requests, &catalog).unwrap();
        let next = transition.next_snapshot;
        let next_active = &next.participants[0].team[0];

        assert!(next_active.energy >= charged_cost);
        assert_eq!(
            next.timeline[0].events[0].move_id.as_deref(),
            Some("TACKLE")
        );

        let next_turn_requests = [
            ActionRequest {
                participant_id: "left".to_owned(),
                turn: next.turn,
                action: PlayerAction::ChargedMove {
                    move_id: "POWER_WHIP".to_owned(),
                },
            },
            ActionRequest {
                participant_id: "right".to_owned(),
                turn: next.turn,
                action: PlayerAction::Wait,
            },
        ];
        let next_turn_normalized = normalize_actions(&next, &next_turn_requests, &catalog);

        assert_eq!(
            next_turn_normalized[0].action,
            PlayerAction::ChargedMove {
                move_id: "POWER_WHIP".to_owned()
            }
        );
        assert_eq!(next_turn_normalized[0].fallback_reason, None);
        assert_eq!(state.participants[0].team[0].energy, charged_cost - 1);
    }

    #[test]
    fn charged_move_shield_decision_reduces_damage_to_one() {
        let mut state = snapshot();
        let catalog = DataCatalog::embedded().unwrap();
        let charged_cost = catalog.charged_move_cost("POWER_WHIP").unwrap();
        let attacker = &mut state.participants[0].team[0];
        attacker.energy = charged_cost;
        attacker
            .move_state
            .charged_moves
            .push("POWER_WHIP".to_owned());
        state.participants[1].shields_remaining = 1;
        let actions = [
            ActionRequest {
                participant_id: "left".to_owned(),
                turn: 4,
                action: PlayerAction::ChargedMove {
                    move_id: "POWER_WHIP".to_owned(),
                },
            },
            ActionRequest {
                participant_id: "right".to_owned(),
                turn: 4,
                action: PlayerAction::Wait,
            },
        ];
        let queued = resolve_turn(&state, &actions, &catalog).unwrap();
        let queued_state = queued.next_snapshot;
        assert_eq!(queued_state.participants[1].team[0].current_hp, 100);
        assert_eq!(queued_state.participants[0].team[0].energy, 0);
        assert_eq!(queued_state.pending_charged_moves.len(), 1);
        assert!(
            queued_state.participants[0].team[0]
                .move_state
                .pending_charge
        );
        let next_turn_actions = [
            ActionRequest {
                participant_id: "left".to_owned(),
                turn: 5,
                action: PlayerAction::FastMove {
                    move_id: "TACKLE".to_owned(),
                },
            },
            ActionRequest {
                participant_id: "right".to_owned(),
                turn: 5,
                action: PlayerAction::Wait,
            },
        ];
        let shield_decisions = [ShieldDecisionRequest {
            participant_id: "right".to_owned(),
            turn: 5,
            decision: ShieldDecision::Shield,
        }];

        let shielded = resolve_turn_with_shield_decisions(
            &queued_state,
            &next_turn_actions,
            &shield_decisions,
            &catalog,
        )
        .unwrap();
        let shielded_next = shielded.next_snapshot;
        let shielded_event = &shielded_next.timeline[1].events[0];
        assert_eq!(shielded_next.participants[1].team[0].current_hp, 99);
        assert_eq!(shielded_next.participants[1].shields_remaining, 0);
        assert_eq!(shielded_event.kind, TimelineEventKind::Charged);
        assert_eq!(shielded_event.damage, Some(1));
        assert_eq!(shielded_event.shielded, Some(true));
        assert_eq!(shielded_next.timeline[1].events.len(), 1);
        assert!(
            !shielded_next.participants[0].team[0]
                .move_state
                .pending_charge
        );

        let unshielded_queue = resolve_turn(&state, &actions, &catalog).unwrap();
        let unshielded = resolve_turn(
            &unshielded_queue.next_snapshot,
            &next_turn_actions,
            &catalog,
        )
        .unwrap();
        let unshielded_next = unshielded.next_snapshot;
        let unshielded_event = &unshielded_next.timeline[1].events[0];
        assert!(unshielded_next.participants[1].team[0].current_hp < 99);
        assert_eq!(unshielded_next.participants[1].shields_remaining, 1);
        assert_eq!(unshielded_event.shielded, Some(false));
        assert!(unshielded_event.damage.unwrap() > 1);
        assert_eq!(
            unshielded_next.timeline[1].events[1].kind,
            TimelineEventKind::Faint
        );
        assert!(
            unshielded_next.timeline[1]
                .events
                .iter()
                .all(|event| event.kind != TimelineEventKind::TapInteraction)
        );
    }

    #[test]
    fn higher_attack_charged_move_resolves_first_and_cancels_fainted_move() {
        let mut state = snapshot();
        let catalog = DataCatalog::embedded().unwrap();
        let charged_cost = catalog.charged_move_cost("POWER_WHIP").unwrap();
        let left = &mut state.participants[0].team[0];
        left.current_hp = 1;
        left.energy = charged_cost;
        left.move_state.charged_moves.push("POWER_WHIP".to_owned());
        let right = &mut state.participants[1].team[0];
        right.energy = charged_cost;
        right.combat_stats.attack = 200.0;
        right.move_state.charged_moves.push("POWER_WHIP".to_owned());
        let requests = [
            ActionRequest {
                participant_id: "left".to_owned(),
                turn: 4,
                action: PlayerAction::ChargedMove {
                    move_id: "POWER_WHIP".to_owned(),
                },
            },
            ActionRequest {
                participant_id: "right".to_owned(),
                turn: 4,
                action: PlayerAction::ChargedMove {
                    move_id: "POWER_WHIP".to_owned(),
                },
            },
        ];

        let queued_transition = resolve_turn(&state, &requests, &catalog).unwrap();
        let queued = queued_transition.next_snapshot;
        assert_eq!(queued.pending_charged_moves.len(), 2);
        assert_eq!(queued.participants[0].team[0].current_hp, 1);
        assert_eq!(queued.participants[1].team[0].current_hp, 100);
        assert_eq!(queued.participants[0].team[0].energy, 0);
        assert_eq!(queued.participants[1].team[0].energy, 0);

        let next_turn_requests = [
            ActionRequest {
                participant_id: "left".to_owned(),
                turn: 5,
                action: PlayerAction::Wait,
            },
            ActionRequest {
                participant_id: "right".to_owned(),
                turn: 5,
                action: PlayerAction::Wait,
            },
        ];
        let transition = resolve_turn(&queued, &next_turn_requests, &catalog).unwrap();
        let next = transition.next_snapshot;
        let charged_events: Vec<_> = next.timeline[1]
            .events
            .iter()
            .filter(|event| event.kind == TimelineEventKind::Charged)
            .collect();

        assert_eq!(charged_events.len(), 1);
        assert_eq!(charged_events[0].participant_id, "right");
        assert!(next.participants[0].team[0].flags.fainted);
        assert_eq!(next.participants[1].team[0].energy, 0);
        assert!(next.pending_charged_moves.is_empty());
        assert_eq!(next.timeline[1].events[1].kind, TimelineEventKind::Faint);
    }

    #[test]
    fn charged_move_is_cancelled_if_its_user_faints_before_it_starts() {
        let mut state = snapshot();
        let catalog = DataCatalog::embedded().unwrap();
        let charged_cost = catalog.charged_move_cost("POWER_WHIP").unwrap();
        let attacker = &mut state.participants[0].team[0];
        attacker.current_hp = 1;
        attacker.energy = charged_cost;
        attacker
            .move_state
            .charged_moves
            .push("POWER_WHIP".to_owned());
        let requests = [
            ActionRequest {
                participant_id: "left".to_owned(),
                turn: 4,
                action: PlayerAction::ChargedMove {
                    move_id: "POWER_WHIP".to_owned(),
                },
            },
            ActionRequest {
                participant_id: "right".to_owned(),
                turn: 4,
                action: PlayerAction::FastMove {
                    move_id: "TACKLE".to_owned(),
                },
            },
        ];

        let transition = resolve_turn(&state, &requests, &catalog).unwrap();
        let next = transition.next_snapshot;

        assert!(next.participants[0].team[0].flags.fainted);
        assert!(next.pending_charged_moves.is_empty());
        assert!(!next.participants[0].team[0].move_state.pending_charge);
        assert!(
            next.timeline[0]
                .events
                .iter()
                .all(|event| event.kind != TimelineEventKind::Charged)
        );
    }

    #[test]
    fn voluntary_swap_happens_before_incoming_fast_move_damage() {
        let mut state = snapshot();
        let mut bench = pokemon(1, "charmander");
        bench.is_active = false;
        state.participants[0].team[0]
            .stat_stages
            .apply_attack_delta(2);
        state.participants[0].team.push(bench);
        let requests = [
            ActionRequest {
                participant_id: "left".to_owned(),
                turn: 4,
                action: PlayerAction::Swap { target_slot: 1 },
            },
            ActionRequest {
                participant_id: "right".to_owned(),
                turn: 4,
                action: PlayerAction::FastMove {
                    move_id: "TACKLE".to_owned(),
                },
            },
        ];
        let catalog = DataCatalog::embedded().unwrap();

        let transition = resolve_turn(&state, &requests, &catalog).unwrap();
        let next = transition.next_snapshot;

        assert_eq!(next.participants[0].active_pokemon_index, 1);
        assert!(!next.participants[0].team[0].is_active);
        assert!(next.participants[0].team[1].is_active);
        assert_eq!(next.participants[0].team[0].stat_stages.attack_stage(), 0);
        assert_eq!(next.participants[0].team[0].current_hp, 100);
        assert_eq!(next.participants[0].team[0].energy, 20);
        assert_eq!(next.participants[0].team[1].current_hp, 98);
        assert_eq!(next.participants[1].team[0].current_hp, 100);
        assert_eq!(next.timeline[0].events[0].kind, TimelineEventKind::Swap);
        assert_eq!(
            next.timeline[0].events[0].species.as_deref(),
            Some("charmander")
        );
        assert_eq!(
            next.timeline[0].events[1].kind,
            TimelineEventKind::TapInteraction
        );
        assert_eq!(state.participants[0].active_pokemon_index, 0);
        assert_eq!(state.participants[0].team[0].stat_stages.attack_stage(), 2);
    }

    #[test]
    fn fainted_active_is_forced_to_the_next_living_bench_pokemon() {
        let mut state = snapshot();
        state.participants[0].team[0].current_hp = 1;
        let mut bench = pokemon(1, "charmander");
        bench.is_active = false;
        state.participants[0].team.push(bench);
        let requests = [
            ActionRequest {
                participant_id: "left".to_owned(),
                turn: 4,
                action: PlayerAction::Wait,
            },
            ActionRequest {
                participant_id: "right".to_owned(),
                turn: 4,
                action: PlayerAction::FastMove {
                    move_id: "TACKLE".to_owned(),
                },
            },
        ];
        let catalog = DataCatalog::embedded().unwrap();

        let transition = resolve_turn(&state, &requests, &catalog).unwrap();
        let next = transition.next_snapshot;
        let events = &next.timeline[0].events;

        assert_eq!(next.turn, 5);
        assert_eq!(next.participants[0].active_pokemon_index, 1);
        assert_eq!(next.participants[0].team[0].current_hp, 0);
        assert!(next.participants[0].team[0].flags.fainted);
        assert!(!next.participants[0].team[0].is_active);
        assert!(next.participants[0].team[1].is_active);
        assert_eq!(events[1].kind, TimelineEventKind::Faint);
        assert_eq!(events[1].species.as_deref(), Some("bulbasaur"));
        assert_eq!(events[2].kind, TimelineEventKind::Swap);
        assert_eq!(events[2].species.as_deref(), Some("charmander"));
        assert_eq!(state.participants[0].team[0].current_hp, 1);
        assert!(next.pending_forced_swaps.is_empty());
    }

    #[test]
    fn trainer_selects_forced_replacement_when_multiple_bench_pokemon_remain() {
        let mut state = snapshot();
        state.participants[0].team[0].current_hp = 1;
        let mut charmander = pokemon(1, "charmander");
        charmander.is_active = false;
        let mut wartortle = pokemon(2, "wartortle");
        wartortle.is_active = false;
        state.participants[0].team.extend([charmander, wartortle]);
        let requests = [
            ActionRequest {
                participant_id: "left".to_owned(),
                turn: 4,
                action: PlayerAction::Wait,
            },
            ActionRequest {
                participant_id: "right".to_owned(),
                turn: 4,
                action: PlayerAction::FastMove {
                    move_id: "TACKLE".to_owned(),
                },
            },
        ];
        let catalog = DataCatalog::embedded().unwrap();

        let faint_transition = resolve_turn(&state, &requests, &catalog).unwrap();
        let pending = faint_transition.next_snapshot;

        assert_eq!(pending.turn, 5);
        assert_eq!(pending.participants[0].active_pokemon_index, 0);
        assert!(!pending.participants[0].team[0].is_active);
        assert_eq!(pending.pending_forced_swaps.len(), 1);
        assert_eq!(pending.pending_forced_swaps[0].options, vec![1, 2]);
        assert_eq!(pending.timeline[0].events[1].kind, TimelineEventKind::Faint);
        assert_eq!(pending.timeline[0].events.len(), 2);

        let choice = ActionRequest {
            participant_id: "left".to_owned(),
            turn: 5,
            action: PlayerAction::Swap { target_slot: 2 },
        };
        let choice_transition = resolve_turn(&pending, &[choice], &catalog).unwrap();
        let selected = choice_transition.next_snapshot;

        assert_eq!(selected.turn, 5);
        assert_eq!(selected.participants[0].active_pokemon_index, 2);
        assert!(selected.participants[0].team[2].is_active);
        assert!(selected.pending_forced_swaps.is_empty());
        assert_eq!(selected.timeline.len(), 1);
        assert_eq!(selected.timeline[0].turn, 4);
        assert_eq!(selected.timeline[0].events[2].kind, TimelineEventKind::Swap);
        assert_eq!(
            selected.timeline[0].events[2].species.as_deref(),
            Some("wartortle")
        );
        assert_eq!(choice_transition.timeline_entries.len(), 1);
    }
}
