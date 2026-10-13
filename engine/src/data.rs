use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Deserializer};

use crate::model::{CombatStats, PokemonBuildInput};

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveDefinition {
    pub move_id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub move_type: PokemonType,
    pub power: u32,
    pub energy: u32,
    pub energy_gain: u32,
    pub cooldown: u32,
    pub turns: u32,
    #[serde(default)]
    pub buffs: Option<[i8; 2]>,
    #[serde(default)]
    pub buff_target: Option<BuffTarget>,
    #[serde(default, deserialize_with = "deserialize_buff_apply_chance")]
    pub buff_apply_chance: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuffTarget {
    #[serde(rename = "self")]
    SelfOnly,
    Opponent,
    Both,
}

fn deserialize_buff_apply_chance<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let serialized_chance = Option::<String>::deserialize(deserializer)?;
    let Some(serialized_chance) = serialized_chance else {
        return Ok(None);
    };
    let chance = serialized_chance
        .parse::<f64>()
        .map_err(serde::de::Error::custom)?;
    if !chance.is_finite() || !(0.0..=1.0).contains(&chance) {
        return Err(serde::de::Error::custom(
            "buff apply chance must be between zero and one",
        ));
    }
    Ok(Some(chance))
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseStats {
    pub atk: u32,
    pub def: u32,
    pub hp: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PokemonDefinition {
    pub dex: u16,
    pub species_id: String,
    pub species_name: String,
    pub base_stats: BaseStats,
    pub types: Vec<PokemonType>,
    pub fast_moves: Vec<String>,
    pub charged_moves: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PokemonType {
    Normal,
    Fighting,
    Flying,
    Poison,
    Ground,
    Rock,
    Bug,
    Ghost,
    Steel,
    Fire,
    Water,
    Grass,
    Electric,
    Psychic,
    Ice,
    Dragon,
    Dark,
    Fairy,
    None,
}

impl PokemonType {
    const ALL: [Self; 18] = [
        Self::Normal,
        Self::Fighting,
        Self::Flying,
        Self::Poison,
        Self::Ground,
        Self::Rock,
        Self::Bug,
        Self::Ghost,
        Self::Steel,
        Self::Fire,
        Self::Water,
        Self::Grass,
        Self::Electric,
        Self::Psychic,
        Self::Ice,
        Self::Dragon,
        Self::Dark,
        Self::Fairy,
    ];
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct TypeTraits {
    pub resistances: Vec<PokemonType>,
    pub weaknesses: Vec<PokemonType>,
    pub immunities: Vec<PokemonType>,
}

#[derive(Clone, Copy, Debug)]
pub struct DamageConstants;

impl DamageConstants {
    pub const BONUS: f64 = 1.2999999523162841796875;
    pub const SUPER_EFFECTIVE: f64 = 1.60000002384185791015625;
    pub const RESISTED: f64 = 0.625;
    pub const DOUBLE_RESISTED: f64 = 0.390625;
    pub const STAB: f64 = 1.2000000476837158203125;
    pub const SHADOW_ATK: f64 = 1.2;
    pub const SHADOW_DEF: f64 = 0.83333331;
}

#[derive(Clone, Debug, PartialEq)]
pub struct InitializedPokemonStats {
    pub combat_stats: CombatStats,
    pub max_hp: u32,
    pub cp: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatInitializationError {
    UnknownSpecies,
    InvalidIv,
    UnsupportedLevel,
}

impl Display for StatInitializationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownSpecies => formatter.write_str("unknown species id"),
            Self::InvalidIv => formatter.write_str("IV values must be between 0 and 15"),
            Self::UnsupportedLevel => formatter.write_str("level has no CPM table entry"),
        }
    }
}

impl Error for StatInitializationError {}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CpmTable {
    starting_level: f64,
    level_step: f64,
    values: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct DataCatalog {
    moves: HashMap<String, MoveDefinition>,
    pokemon: HashMap<String, PokemonDefinition>,
    cpm_table: CpmTable,
    type_traits: HashMap<PokemonType, TypeTraits>,
}

#[derive(Debug)]
pub struct DataCatalogError(String);

impl Display for DataCatalogError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for DataCatalogError {}

impl DataCatalog {
    pub fn embedded() -> Result<Self, DataCatalogError> {
        Self::from_json(
            include_str!("../data/moves.json"),
            include_str!("../data/pokemon.json"),
            include_str!("../data/cpm.json"),
            include_str!("../data/type_chart.json"),
        )
    }

    pub fn from_json(
        moves_json: &str,
        pokemon_json: &str,
        cpm_json: &str,
        type_chart_json: &str,
    ) -> Result<Self, DataCatalogError> {
        let move_definitions: Vec<MoveDefinition> = serde_json::from_str(moves_json)
            .map_err(|error| DataCatalogError(format!("invalid move data: {error}")))?;
        let pokemon_definitions: Vec<PokemonDefinition> = serde_json::from_str(pokemon_json)
            .map_err(|error| DataCatalogError(format!("invalid Pokemon data: {error}")))?;
        let cpm_table: CpmTable = serde_json::from_str(cpm_json)
            .map_err(|error| DataCatalogError(format!("invalid CPM data: {error}")))?;
        let type_traits: HashMap<PokemonType, TypeTraits> =
            serde_json::from_str(type_chart_json)
                .map_err(|error| DataCatalogError(format!("invalid type chart data: {error}")))?;

        if !cpm_table.starting_level.is_finite()
            || !cpm_table.level_step.is_finite()
            || cpm_table.level_step <= 0.0
            || cpm_table.values.is_empty()
            || cpm_table
                .values
                .iter()
                .any(|value| !value.is_finite() || *value <= 0.0)
        {
            return Err(DataCatalogError("invalid CPM table values".to_owned()));
        }
        validate_type_chart(&type_traits)?;

        let moves = unique_by_key(move_definitions, |definition| definition.move_id.clone())
            .map_err(|key| DataCatalogError(format!("duplicate move id: {key}")))?;
        validate_move_buffs(moves.values())?;
        let pokemon = unique_by_key(pokemon_definitions, |definition| {
            definition.species_id.clone()
        })
        .map_err(|key| DataCatalogError(format!("duplicate species id: {key}")))?;

        Ok(Self {
            moves,
            pokemon,
            cpm_table,
            type_traits,
        })
    }

    pub fn move_definition(&self, move_id: &str) -> Option<&MoveDefinition> {
        self.moves.get(move_id)
    }

    pub fn pokemon_definition(&self, species_id: &str) -> Option<&PokemonDefinition> {
        self.pokemon.get(species_id)
    }

    pub fn charged_move_cost(&self, move_id: &str) -> Option<u32> {
        self.move_definition(move_id)
            .filter(|definition| definition.energy > 0)
            .map(|definition| definition.energy)
    }

    pub fn type_effectiveness(
        &self,
        move_type: PokemonType,
        target_types: &[PokemonType],
    ) -> Result<f64, DataCatalogError> {
        let mut effectiveness = 1.0;

        for target_type in target_types {
            if *target_type == PokemonType::None {
                continue;
            }

            let traits = self.type_traits.get(target_type).ok_or_else(|| {
                DataCatalogError(format!("missing type traits for {target_type:?}"))
            })?;

            if traits.weaknesses.contains(&move_type) {
                effectiveness *= DamageConstants::SUPER_EFFECTIVE;
            } else if traits.resistances.contains(&move_type) {
                effectiveness *= DamageConstants::RESISTED;
            } else if traits.immunities.contains(&move_type) {
                effectiveness *= DamageConstants::DOUBLE_RESISTED;
            }
        }

        Ok(effectiveness)
    }

    pub fn cp_multiplier(&self, level: f64) -> Option<f64> {
        if !level.is_finite() {
            return None;
        }

        let index = (level - self.cpm_table.starting_level) / self.cpm_table.level_step;
        let nearest_index = index.round();
        if index < 0.0 || (index - nearest_index).abs() > 1e-9 {
            return None;
        }

        self.cpm_table.values.get(nearest_index as usize).copied()
    }

    pub fn initialize_stats(
        &self,
        input: &PokemonBuildInput,
    ) -> Result<InitializedPokemonStats, StatInitializationError> {
        if input.ivs.attack > 15 || input.ivs.defense > 15 || input.ivs.hp > 15 {
            return Err(StatInitializationError::InvalidIv);
        }

        let pokemon = self
            .pokemon_definition(&input.species_id)
            .ok_or(StatInitializationError::UnknownSpecies)?;
        let cpm = self
            .cp_multiplier(f64::from(input.level))
            .ok_or(StatInitializationError::UnsupportedLevel)?;
        let attack = cpm * f64::from(pokemon.base_stats.atk + u32::from(input.ivs.attack));
        let defense = cpm * f64::from(pokemon.base_stats.def + u32::from(input.ivs.defense));
        let stamina = pokemon.base_stats.hp + u32::from(input.ivs.hp);
        let max_hp = if pokemon.dex == 292 {
            10
        } else {
            (cpm * f64::from(stamina)).floor().max(10.0) as u32
        };
        let cp = (attack
            * f64::from(pokemon.base_stats.def + u32::from(input.ivs.defense)).sqrt()
            * f64::from(stamina).sqrt()
            * cpm
            / 10.0)
            .floor()
            .max(10.0) as u32;

        Ok(InitializedPokemonStats {
            combat_stats: CombatStats { attack, defense },
            max_hp,
            cp,
        })
    }
}

fn validate_move_buffs<'a>(
    moves: impl IntoIterator<Item = &'a MoveDefinition>,
) -> Result<(), DataCatalogError> {
    for move_definition in moves {
        let has_buffs = move_definition.buffs.is_some();
        let has_target = move_definition.buff_target.is_some();
        let has_chance = move_definition.buff_apply_chance.is_some();
        if has_buffs != has_target || has_buffs != has_chance {
            return Err(DataCatalogError(format!(
                "move {} must provide buffs, buffTarget, and buffApplyChance together",
                move_definition.move_id
            )));
        }
    }
    Ok(())
}

fn unique_by_key<T>(
    definitions: Vec<T>,
    key_for: impl Fn(&T) -> String,
) -> Result<HashMap<String, T>, String> {
    let mut by_key = HashMap::with_capacity(definitions.len());
    for definition in definitions {
        let key = key_for(&definition);
        if by_key.insert(key.clone(), definition).is_some() {
            return Err(key);
        }
    }
    Ok(by_key)
}

fn validate_type_chart(
    type_traits: &HashMap<PokemonType, TypeTraits>,
) -> Result<(), DataCatalogError> {
    if type_traits.len() != PokemonType::ALL.len()
        || PokemonType::ALL
            .iter()
            .any(|pokemon_type| !type_traits.contains_key(pokemon_type))
    {
        return Err(DataCatalogError(
            "type chart must define each of the 18 Pokemon types exactly once".to_owned(),
        ));
    }

    for (defending_type, traits) in type_traits {
        let mut seen = HashSet::new();
        for attacking_type in traits
            .weaknesses
            .iter()
            .chain(&traits.resistances)
            .chain(&traits.immunities)
        {
            if !seen.insert(*attacking_type) {
                return Err(DataCatalogError(format!(
                    "duplicate or conflicting {attacking_type:?} trait for {defending_type:?}"
                )));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::model::PokemonBuildInput;

    use super::{BuffTarget, DamageConstants, DataCatalog, PokemonType, StatInitializationError};

    #[test]
    fn embedded_catalog_loads_move_costs_and_pokemon_stats() {
        let catalog = DataCatalog::embedded().unwrap();

        let vine_whip = catalog.move_definition("VINE_WHIP").unwrap();
        assert_eq!(vine_whip.energy_gain, 8);
        assert_eq!(vine_whip.turns, 2);

        let icy_wind = catalog.move_definition("ICY_WIND").unwrap();
        assert_eq!(icy_wind.buffs, Some([-1, 0]));
        assert_eq!(icy_wind.buff_target, Some(BuffTarget::Opponent));
        assert_eq!(icy_wind.buff_apply_chance, Some(1.0));

        let bulbasaur = catalog.pokemon_definition("bulbasaur").unwrap();
        assert_eq!(bulbasaur.base_stats.atk, 118);
        assert_eq!(bulbasaur.types, [PokemonType::Grass, PokemonType::Poison]);
        assert_eq!(
            catalog.pokemon_definition("charmander").unwrap().types,
            [PokemonType::Fire, PokemonType::None]
        );
        assert_eq!(catalog.charged_move_cost("POWER_WHIP"), Some(50));
        assert_eq!(catalog.cp_multiplier(1.0), Some(0.0939999967813491));
        assert_eq!(catalog.cp_multiplier(1.5), Some(0.135137430784308));
        assert_eq!(catalog.cp_multiplier(2.0), Some(0.166397869586944));
        assert_eq!(catalog.cpm_table.values.len(), 109);
        assert_eq!(catalog.cp_multiplier(55.0), Some(0.865299999713897));
        assert_eq!(catalog.cp_multiplier(55.5), None);
        assert_eq!(catalog.cp_multiplier(1.25), None);
    }

    #[test]
    fn type_effectiveness_matches_pvpoke_single_and_dual_type_rules() {
        let catalog = DataCatalog::embedded().unwrap();

        assert_eq!(
            catalog
                .type_effectiveness(PokemonType::Fighting, &[PokemonType::Normal])
                .unwrap(),
            DamageConstants::SUPER_EFFECTIVE
        );
        assert_eq!(
            catalog
                .type_effectiveness(PokemonType::Ghost, &[PokemonType::Normal])
                .unwrap(),
            DamageConstants::DOUBLE_RESISTED
        );
        assert_eq!(
            catalog
                .type_effectiveness(PokemonType::Water, &[PokemonType::Fire, PokemonType::Grass])
                .unwrap(),
            DamageConstants::SUPER_EFFECTIVE * DamageConstants::RESISTED
        );
        assert_eq!(
            catalog
                .type_effectiveness(PokemonType::Flying, &[PokemonType::Grass, PokemonType::Bug])
                .unwrap(),
            DamageConstants::SUPER_EFFECTIVE.powi(2)
        );
        assert_eq!(
            catalog
                .type_effectiveness(PokemonType::Ground, &[PokemonType::None])
                .unwrap(),
            1.0
        );
    }

    #[test]
    fn build_input_derives_battle_stats_from_species_ivs_and_level() {
        let catalog = DataCatalog::embedded().unwrap();
        let input = PokemonBuildInput {
            species_id: "bulbasaur".to_owned(),
            level: 1.0,
            ivs: crate::model::IndividualValues {
                attack: 10,
                defense: 11,
                hp: 12,
            },
            shadow: false,
        };

        let stats = catalog.initialize_stats(&input).unwrap();

        assert_eq!(stats.combat_stats.attack, 0.0939999967813491 * 128.0);
        assert_eq!(stats.combat_stats.defense, 0.0939999967813491 * 122.0);
        assert_eq!(stats.max_hp, 13);
        assert_eq!(stats.cp, 14);
    }

    #[test]
    fn build_input_rejects_invalid_ivs_and_levels() {
        let catalog = DataCatalog::embedded().unwrap();
        let mut input = PokemonBuildInput {
            species_id: "bulbasaur".to_owned(),
            level: 1.25,
            ivs: crate::model::IndividualValues {
                attack: 16,
                defense: 0,
                hp: 0,
            },
            shadow: false,
        };

        assert_eq!(
            catalog.initialize_stats(&input),
            Err(StatInitializationError::InvalidIv)
        );

        input.ivs.attack = 0;
        assert_eq!(
            catalog.initialize_stats(&input),
            Err(StatInitializationError::UnsupportedLevel)
        );
    }

    #[test]
    fn duplicate_move_ids_are_rejected() {
        let moves = r#"[
            {"moveId":"DUP","name":"One","type":"normal","power":1,"energy":1,"energyGain":0,"cooldown":500,"turns":1},
            {"moveId":"DUP","name":"Two","type":"normal","power":1,"energy":1,"energyGain":0,"cooldown":500,"turns":1}
        ]"#;
        let pokemon = "[]";

        let cpm = r#"{"startingLevel":1.0,"levelStep":0.5,"values":[0.1]}"#;
        let type_chart = include_str!("../data/type_chart.json");
        let error = DataCatalog::from_json(moves, pokemon, cpm, type_chart).unwrap_err();

        assert_eq!(error.to_string(), "duplicate move id: DUP");
    }

    #[test]
    fn move_buff_fields_must_be_complete() {
        let moves = r#"[
            {"moveId":"PARTIAL","name":"Partial","type":"normal","power":1,"energy":1,"energyGain":0,"cooldown":500,"turns":1,"buffs":[-1,0]}
        ]"#;
        let cpm = r#"{"startingLevel":1.0,"levelStep":0.5,"values":[0.1]}"#;
        let type_chart = include_str!("../data/type_chart.json");

        let error = DataCatalog::from_json(moves, "[]", cpm, type_chart).unwrap_err();

        assert_eq!(
            error.to_string(),
            "move PARTIAL must provide buffs, buffTarget, and buffApplyChance together"
        );
    }
}
