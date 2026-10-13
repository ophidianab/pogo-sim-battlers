use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::data::{DamageConstants, DataCatalog, MoveDefinition, PokemonType};
use crate::model::{CombatStats, StatStages};

pub struct DamageCombatant<'a> {
    pub combat_stats: &'a CombatStats,
    pub stat_stages: &'a StatStages,
    pub shadow: bool,
    pub types: &'a [PokemonType],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DamageCalculationError {
    InvalidAttackStat,
    InvalidDefenseStat,
    InvalidChargeMultiplier,
    TypeChart(String),
}

impl Display for DamageCalculationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAttackStat => {
                formatter.write_str("attack stat must be finite and positive")
            }
            Self::InvalidDefenseStat => {
                formatter.write_str("defense stat must be finite and positive")
            }
            Self::InvalidChargeMultiplier => {
                formatter.write_str("charge multiplier must be finite and non-negative")
            }
            Self::TypeChart(error) => formatter.write_str(error),
        }
    }
}

impl Error for DamageCalculationError {}

pub fn calculate_damage(
    catalog: &DataCatalog,
    attacker: DamageCombatant<'_>,
    defender: DamageCombatant<'_>,
    move_definition: &MoveDefinition,
    charge_multiplier: f64,
) -> Result<u32, DamageCalculationError> {
    if !attacker.combat_stats.attack.is_finite() || attacker.combat_stats.attack <= 0.0 {
        return Err(DamageCalculationError::InvalidAttackStat);
    }
    if !defender.combat_stats.defense.is_finite() || defender.combat_stats.defense <= 0.0 {
        return Err(DamageCalculationError::InvalidDefenseStat);
    }
    if !charge_multiplier.is_finite() || charge_multiplier < 0.0 {
        return Err(DamageCalculationError::InvalidChargeMultiplier);
    }

    let effectiveness = catalog
        .type_effectiveness(move_definition.move_type, defender.types)
        .map_err(|error| DamageCalculationError::TypeChart(error.to_string()))?;
    let stab = if attacker.types.contains(&move_definition.move_type) {
        DamageConstants::STAB
    } else {
        1.0
    };
    let shadow_attack = if attacker.shadow {
        DamageConstants::SHADOW_ATK
    } else {
        1.0
    };
    let shadow_defense = if defender.shadow {
        DamageConstants::SHADOW_DEF
    } else {
        1.0
    };
    let attack_stat =
        attacker.combat_stats.attack * attacker.stat_stages.attack_multiplier() * shadow_attack;
    let defense_stat =
        defender.combat_stats.defense * defender.stat_stages.defense_multiplier() * shadow_defense;

    let damage = (f64::from(move_definition.power)
        * stab
        * (attack_stat / defense_stat)
        * effectiveness
        * charge_multiplier
        * 0.5
        * DamageConstants::BONUS)
        .floor()
        + 1.0;

    Ok(damage as u32)
}

#[cfg(test)]
mod tests {
    use crate::data::{DataCatalog, MoveDefinition, PokemonType};
    use crate::model::{CombatStats, StatStages};

    use super::{DamageCalculationError, DamageCombatant, calculate_damage};

    fn move_definition(move_type: PokemonType, power: u32) -> MoveDefinition {
        MoveDefinition {
            move_id: "TEST_MOVE".to_owned(),
            name: "Test Move".to_owned(),
            move_type,
            power,
            energy: 0,
            energy_gain: 0,
            cooldown: 500,
            turns: 1,
            buffs: None,
            buff_target: None,
            buffs_self: None,
            buffs_opponent: None,
            buff_apply_chance: None,
        }
    }

    #[test]
    fn neutral_damage_matches_pvpoke_formula_rounding() {
        let catalog = DataCatalog::embedded().unwrap();
        let attacker_types = [PokemonType::Fire];
        let defender_types = [PokemonType::Fire];
        let attacker_stats = CombatStats {
            attack: 100.0,
            defense: 100.0,
        };
        let attacker_stages = StatStages::default();
        let defender_stats = CombatStats {
            attack: 100.0,
            defense: 100.0,
        };
        let defender_stages = StatStages::default();
        let damage = calculate_damage(
            &catalog,
            DamageCombatant {
                combat_stats: &attacker_stats,
                stat_stages: &attacker_stages,
                shadow: false,
                types: &attacker_types,
            },
            DamageCombatant {
                combat_stats: &defender_stats,
                stat_stages: &defender_stages,
                shadow: false,
                types: &defender_types,
            },
            &move_definition(PokemonType::Normal, 50),
            1.0,
        )
        .unwrap();

        assert_eq!(damage, 33);
    }

    #[test]
    fn damage_uses_stab_effectiveness_stages_shadow_and_charge() {
        let catalog = DataCatalog::embedded().unwrap();
        let attacker_types = [PokemonType::Grass];
        let defender_types = [PokemonType::Water];
        let mut attacker_stages = StatStages::default();
        attacker_stages.apply_attack_delta(2);
        let attacker_stats = CombatStats {
            attack: 100.0,
            defense: 100.0,
        };
        let defender_stats = CombatStats {
            attack: 100.0,
            defense: 100.0,
        };
        let defender_stages = StatStages::default();

        let damage = calculate_damage(
            &catalog,
            DamageCombatant {
                combat_stats: &attacker_stats,
                stat_stages: &attacker_stages,
                shadow: true,
                types: &attacker_types,
            },
            DamageCombatant {
                combat_stats: &defender_stats,
                stat_stages: &defender_stages,
                shadow: true,
                types: &defender_types,
            },
            &move_definition(PokemonType::Grass, 40),
            0.5,
        )
        .unwrap();

        assert_eq!(damage, 54);
    }

    #[test]
    fn damage_rejects_invalid_defense_and_charge_multiplier() {
        let catalog = DataCatalog::embedded().unwrap();
        let types = [PokemonType::Normal];
        let attacker_stats = CombatStats {
            attack: 100.0,
            defense: 100.0,
        };
        let attacker_stages = StatStages::default();
        let defender_stats = CombatStats {
            attack: 100.0,
            defense: 0.0,
        };
        let defender_stages = StatStages::default();
        let move_definition = move_definition(PokemonType::Normal, 50);

        let invalid_defense = calculate_damage(
            &catalog,
            DamageCombatant {
                combat_stats: &attacker_stats,
                stat_stages: &attacker_stages,
                shadow: false,
                types: &types,
            },
            DamageCombatant {
                combat_stats: &defender_stats,
                stat_stages: &defender_stages,
                shadow: false,
                types: &types,
            },
            &move_definition,
            1.0,
        );
        assert_eq!(
            invalid_defense,
            Err(DamageCalculationError::InvalidDefenseStat)
        );

        let valid_defender_stats = CombatStats {
            attack: 100.0,
            defense: 100.0,
        };
        let invalid_charge = calculate_damage(
            &catalog,
            DamageCombatant {
                combat_stats: &attacker_stats,
                stat_stages: &attacker_stages,
                shadow: false,
                types: &types,
            },
            DamageCombatant {
                combat_stats: &valid_defender_stats,
                stat_stages: &defender_stages,
                shadow: false,
                types: &types,
            },
            &move_definition,
            f64::NAN,
        );
        assert_eq!(
            invalid_charge,
            Err(DamageCalculationError::InvalidChargeMultiplier)
        );
    }
}
