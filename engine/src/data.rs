use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Deserialize;

use crate::model::{CombatStats, PokemonBuildInput};

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveDefinition {
    pub move_id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub move_type: String,
    pub power: u32,
    pub energy: u32,
    pub energy_gain: u32,
    pub cooldown: u32,
    pub turns: u32,
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
    pub types: Vec<String>,
    pub fast_moves: Vec<String>,
    pub charged_moves: Vec<String>,
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
        )
    }

    pub fn from_json(
        moves_json: &str,
        pokemon_json: &str,
        cpm_json: &str,
    ) -> Result<Self, DataCatalogError> {
        let move_definitions: Vec<MoveDefinition> = serde_json::from_str(moves_json)
            .map_err(|error| DataCatalogError(format!("invalid move data: {error}")))?;
        let pokemon_definitions: Vec<PokemonDefinition> = serde_json::from_str(pokemon_json)
            .map_err(|error| DataCatalogError(format!("invalid Pokemon data: {error}")))?;
        let cpm_table: CpmTable = serde_json::from_str(cpm_json)
            .map_err(|error| DataCatalogError(format!("invalid CPM data: {error}")))?;

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

        let moves = unique_by_key(move_definitions, |definition| definition.move_id.clone())
            .map_err(|key| DataCatalogError(format!("duplicate move id: {key}")))?;
        let pokemon = unique_by_key(pokemon_definitions, |definition| {
            definition.species_id.clone()
        })
        .map_err(|key| DataCatalogError(format!("duplicate species id: {key}")))?;

        Ok(Self {
            moves,
            pokemon,
            cpm_table,
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

#[cfg(test)]
mod tests {
    use crate::model::PokemonBuildInput;

    use super::{DataCatalog, StatInitializationError};

    #[test]
    fn embedded_catalog_loads_move_costs_and_pokemon_stats() {
        let catalog = DataCatalog::embedded().unwrap();

        let vine_whip = catalog.move_definition("VINE_WHIP").unwrap();
        assert_eq!(vine_whip.energy_gain, 8);
        assert_eq!(vine_whip.turns, 2);

        let bulbasaur = catalog.pokemon_definition("bulbasaur").unwrap();
        assert_eq!(bulbasaur.base_stats.atk, 118);
        assert_eq!(bulbasaur.types, ["grass", "poison"]);
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
        let error = DataCatalog::from_json(moves, pokemon, cpm).unwrap_err();

        assert_eq!(error.to_string(), "duplicate move id: DUP");
    }
}
