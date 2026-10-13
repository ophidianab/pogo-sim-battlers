use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::actions::normalize_actions;
use crate::damage::{DamageCombatant, calculate_damage};
use crate::data::DataCatalog;
use crate::model::{
    ActionRequest, BattleSnapshot, PendingForcedSwap, PlayerAction, TimelineEvent,
    TimelineEventKind, TurnRecord,
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
    if snapshot.participants.len() != 2 {
        return Err(TransitionError::ParticipantCount);
    }
    if !snapshot.pending_forced_swaps.is_empty() {
        return resolve_pending_forced_swaps(snapshot, requests);
    }

    let normalized = normalize_actions(snapshot, requests, catalog);
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

    let mut pending_moves = Vec::new();

    for (attacker_index, action) in normalized.iter().enumerate() {
        let target_index = 1 - attacker_index;
        let PlayerAction::FastMove { move_id } = &action.action else {
            if matches!(
                action.action,
                PlayerAction::Wait | PlayerAction::Swap { .. }
            ) {
                continue;
            }
            return Err(TransitionError::UnsupportedAction(
                action.participant_id.clone(),
            ));
        };

        let attacker = active_pokemon(&next_snapshot, attacker_index)?;
        let target = active_pokemon(&next_snapshot, target_index)?;
        let move_definition = catalog
            .move_definition(move_id)
            .ok_or_else(|| TransitionError::UnknownMove(move_id.clone()))?;
        let attacker_definition = catalog
            .pokemon_definition(&attacker.species)
            .ok_or_else(|| TransitionError::UnknownSpecies(attacker.species.clone()))?;
        let target_definition = catalog
            .pokemon_definition(&target.species)
            .ok_or_else(|| TransitionError::UnknownSpecies(target.species.clone()))?;
        let damage = calculate_damage(
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
        .map_err(|error| TransitionError::Damage(error.to_string()))?;

        pending_moves.push(PendingFastMove {
            attacker_index,
            target_index,
            damage,
            energy_gain: move_definition.energy_gain,
            event: TimelineEvent {
                kind: TimelineEventKind::TapInteraction,
                participant_id: snapshot.participants[attacker_index].participant_id.clone(),
                target_participant_id: Some(
                    snapshot.participants[target_index].participant_id.clone(),
                ),
                move_id: Some(move_id.clone()),
                damage: Some(damage),
                shielded: None,
                duration_ms: Some(move_definition.cooldown),
                species: None,
            },
        });
    }

    let pending_damage: Vec<_> = pending_moves
        .iter()
        .map(|pending| (pending.target_index, pending.damage))
        .collect();

    for pending in pending_moves {
        let attacker = active_pokemon_mut(&mut next_snapshot, pending.attacker_index)?;
        attacker.energy = attacker.energy.saturating_add(pending.energy_gain);
        timeline_entries.push(pending.event);
    }

    let mut fainted_participants = Vec::new();
    for (target_index, damage) in pending_damage {
        let target = active_pokemon_mut(&mut next_snapshot, target_index)?;
        let fainted = target.current_hp > 0 && target.current_hp <= damage;
        target.current_hp = target.current_hp.saturating_sub(damage);
        if fainted {
            target.flags.fainted = true;
            fainted_participants.push(target_index);
        }
    }

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
    participant.team[outgoing_index].is_active = false;
    participant.team[incoming_index].is_active = true;
    participant.active_pokemon_index = incoming_index;

    (participant_id, incoming_species)
}

#[cfg(test)]
mod tests {
    use crate::data::DataCatalog;
    use crate::model::{
        ActionRequest, BattleSnapshot, CombatStats, ControllerType, Cooldowns, MoveState,
        ParticipantState, PlayerAction, PokemonFlags, PokemonState, Side, StatStages,
        TimelineEventKind,
    };

    use super::resolve_turn;

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
