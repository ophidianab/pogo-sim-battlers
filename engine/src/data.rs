use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Deserialize;

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
    pub species_id: String,
    pub species_name: String,
    pub base_stats: BaseStats,
    pub types: Vec<String>,
    pub fast_moves: Vec<String>,
    pub charged_moves: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct DataCatalog {
    moves: HashMap<String, MoveDefinition>,
    pokemon: HashMap<String, PokemonDefinition>,
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
        )
    }

    pub fn from_json(moves_json: &str, pokemon_json: &str) -> Result<Self, DataCatalogError> {
        let move_definitions: Vec<MoveDefinition> = serde_json::from_str(moves_json)
            .map_err(|error| DataCatalogError(format!("invalid move data: {error}")))?;
        let pokemon_definitions: Vec<PokemonDefinition> = serde_json::from_str(pokemon_json)
            .map_err(|error| DataCatalogError(format!("invalid Pokemon data: {error}")))?;

        let moves = unique_by_key(move_definitions, |definition| definition.move_id.clone())
            .map_err(|key| DataCatalogError(format!("duplicate move id: {key}")))?;
        let pokemon = unique_by_key(pokemon_definitions, |definition| {
            definition.species_id.clone()
        })
        .map_err(|key| DataCatalogError(format!("duplicate species id: {key}")))?;

        Ok(Self { moves, pokemon })
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
    use super::DataCatalog;

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
    }

    #[test]
    fn duplicate_move_ids_are_rejected() {
        let moves = r#"[
            {"moveId":"DUP","name":"One","type":"normal","power":1,"energy":1,"energyGain":0,"cooldown":500,"turns":1},
            {"moveId":"DUP","name":"Two","type":"normal","power":1,"energy":1,"energyGain":0,"cooldown":500,"turns":1}
        ]"#;
        let pokemon = "[]";

        let error = DataCatalog::from_json(moves, pokemon).unwrap_err();

        assert_eq!(error.to_string(), "duplicate move id: DUP");
    }
}
