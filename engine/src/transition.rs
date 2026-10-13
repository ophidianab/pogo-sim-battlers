use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::actions::normalize_actions;
use crate::damage::{DamageCombatant, calculate_damage};
use crate::data::DataCatalog;
use crate::model::{
    ActionRequest, BattleSnapshot, PlayerAction, TimelineEvent, TimelineEventKind, TurnRecord,
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

    let normalized = normalize_actions(snapshot, requests, catalog);
    let mut pending_moves = Vec::new();

    for (attacker_index, action) in normalized.iter().enumerate() {
        let target_index = 1 - attacker_index;
        let attacker = active_pokemon(snapshot, attacker_index)?;

        let PlayerAction::FastMove { move_id } = &action.action else {
            if matches!(action.action, PlayerAction::Wait) {
                continue;
            }
            return Err(TransitionError::UnsupportedAction(
                action.participant_id.clone(),
            ));
        };

        let target = active_pokemon(snapshot, target_index)?;
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

    let mut next_snapshot = snapshot.clone();
    let mut timeline_entries = Vec::new();
    let pending_damage: Vec<_> = pending_moves
        .iter()
        .map(|pending| (pending.target_index, pending.damage))
        .collect();

    for pending in pending_moves {
        let attacker = active_pokemon_mut(&mut next_snapshot, pending.attacker_index)?;
        attacker.energy = attacker.energy.saturating_add(pending.energy_gain);
        timeline_entries.push(pending.event);
    }

    for (target_index, damage) in pending_damage {
        let target = active_pokemon_mut(&mut next_snapshot, target_index)?;
        target.current_hp = target.current_hp.saturating_sub(damage);
        if target.current_hp == 0 {
            target.flags.fainted = true;
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

#[cfg(test)]
mod tests {
    use crate::data::DataCatalog;
    use crate::model::{
        ActionRequest, BattleSnapshot, CombatStats, ControllerType, Cooldowns, MoveState,
        ParticipantState, PlayerAction, PokemonFlags, PokemonState, Side, StatStages,
        TimelineEventKind,
    };

    use super::resolve_turn;

    fn pokemon(species: &str) -> PokemonState {
        PokemonState {
            slot: 0,
            pokemon_id: species.to_owned(),
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
            team: vec![pokemon(species)],
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
}
