use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BattleSnapshot {
    pub battle_id: String,
    pub turn: u32,
    pub participants: Vec<ParticipantState>,
    #[serde(default)]
    pub timeline: Vec<TurnRecord>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantState {
    pub participant_id: String,
    pub controller_type: ControllerType,
    pub side: Side,
    pub active_pokemon_index: usize,
    pub team: Vec<PokemonState>,
    pub shields_remaining: u8,
    pub switch_cooldown_turns: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerType {
    Human,
    Ai,
    Scripted,
    Rl,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PokemonBuildInput {
    pub species_id: String,
    pub level: f32,
    pub ivs: IndividualValues,
    pub shadow: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndividualValues {
    pub attack: u8,
    pub defense: u8,
    pub hp: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PokemonState {
    pub slot: usize,
    pub pokemon_id: String,
    pub species: String,
    pub is_active: bool,
    pub current_hp: u32,
    pub max_hp: u32,
    pub combat_stats: CombatStats,
    pub energy: u32,
    #[serde(default)]
    pub pending_energy_gain: u32,
    #[serde(default)]
    pub status_effects: Vec<StatusEffect>,
    pub cooldowns: Cooldowns,
    pub move_state: MoveState,
    pub flags: PokemonFlags,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CombatStats {
    pub attack: f64,
    pub defense: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusEffect {
    pub kind: StatusKind,
    pub name: String,
    pub stacks: i32,
    pub duration: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusKind {
    Buff,
    Debuff,
    Shield,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cooldowns {
    pub fast_move_ms: u32,
    pub charged_move_ms: u32,
    pub swap_ms: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveState {
    pub fast_move: String,
    pub charged_moves: Vec<String>,
    pub charged_ready: bool,
    pub pending_charge: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PokemonFlags {
    pub fainted: bool,
    pub shielded: bool,
    pub shadow: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRequest {
    pub participant_id: String,
    pub turn: u32,
    #[serde(flatten)]
    pub action: PlayerAction,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "actionType",
    content = "payload",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PlayerAction {
    FastMove { move_id: String },
    ChargedMove { move_id: String },
    Swap { target_slot: usize },
    Wait,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShieldDecision {
    Shield,
    NoShield,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub observation_version: String,
    pub schema_revision: u64,
    pub turn: u32,
    #[serde(rename = "self")]
    pub self_view: SelfObservation,
    pub opponent: OpponentObservation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfObservation {
    pub participant_id: String,
    pub side: Side,
    pub active: ObservedPokemon,
    pub bench: Vec<ObservedPokemon>,
    pub shields: u8,
    pub switch_cooldown_turns: u32,
    pub move_usage_history: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpponentObservation {
    pub participant_id: String,
    pub side: Side,
    pub active: OpponentActiveObservation,
    pub bench_count: usize,
    pub shields: u8,
    pub last_used_move: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedPokemon {
    pub slot: usize,
    pub species: String,
    pub current_hp: u32,
    pub max_hp: u32,
    pub hp_percentage: f32,
    pub energy: u32,
    pub status_effects: Vec<StatusEffect>,
    pub fast_move: String,
    pub charged_moves: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpponentActiveObservation {
    pub species: String,
    pub hp_percentage: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnRecord {
    pub turn: u32,
    pub timestamp: u64,
    pub events: Vec<TimelineEvent>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEvent {
    pub kind: TimelineEventKind,
    pub participant_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_participant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub move_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shielded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub species: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineEventKind {
    TapInteraction,
    Charged,
    SwitchAvailable,
    Faint,
    TapInteractionWait,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BattleResult {
    pub battle_id: String,
    pub outcome: BattleOutcome,
    pub winner: Option<String>,
    pub loser: Option<String>,
    pub participants: Vec<ParticipantResult>,
    pub timeline: Vec<TurnRecord>,
    pub total_turns: u32,
    pub battle_duration_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BattleOutcome {
    Participant0Win,
    Participant1Win,
    Draw,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantResult {
    pub participant_id: String,
    pub side: Side,
    pub rating: u16,
    pub final_hp: u32,
    pub max_hp: u32,
    pub opponent_damage: u32,
    pub opponent_max_hp: u32,
}

#[cfg(test)]
mod tests {
    use super::{ActionRequest, IndividualValues, PlayerAction, PokemonBuildInput};

    #[test]
    fn action_request_uses_contract_field_names_and_keeps_shields_separate() {
        let request = ActionRequest {
            participant_id: "left-player".to_owned(),
            turn: 3,
            action: PlayerAction::ChargedMove {
                move_id: "move-1".to_owned(),
            },
        };

        let json = serde_json::to_value(&request).unwrap();

        assert_eq!(json["participantId"], "left-player");
        assert_eq!(json["turn"], 3);
        assert_eq!(json["actionType"], "chargedMove");
        assert_eq!(json["payload"]["moveId"], "move-1");
        assert!(json["payload"].get("shield").is_none());
    }

    #[test]
    fn pokemon_build_input_serializes_level_ivs_and_shadow_status() {
        let build = PokemonBuildInput {
            species_id: "bulbasaur".to_owned(),
            level: 20.5,
            ivs: IndividualValues {
                attack: 10,
                defense: 11,
                hp: 12,
            },
            shadow: false,
        };

        let json = serde_json::to_value(build).unwrap();

        assert_eq!(json["speciesId"], "bulbasaur");
        assert_eq!(json["level"], 20.5);
        assert_eq!(json["ivs"]["attack"], 10);
        assert_eq!(json["shadow"], false);
    }
}
