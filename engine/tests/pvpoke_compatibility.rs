use std::fs;
use std::path::Path;

use serde_json::Value;

use engine::battle::{BattleRunner, BattleSetup, ParticipantSetup, PokemonSetup};
use engine::data::DataCatalog;
use engine::model::{ActionRequest, ControllerType, PokemonBuildInput, ShieldDecisionRequest};

#[test]
fn committed_pvpoke_fixtures_match_battle_results() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pvpoke");
    let mut fixture_paths: Vec<_> = fs::read_dir(fixture_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    fixture_paths.sort();
    assert!(
        !fixture_paths.is_empty(),
        "no committed PvPoke fixtures found"
    );

    let catalog = DataCatalog::embedded().unwrap();
    for fixture_path in fixture_paths {
        let fixture: Value = serde_json::from_slice(&fs::read(&fixture_path).unwrap()).unwrap();
        assert_eq!(fixture["fixtureVersion"], 1, "{}", fixture_path.display());
        let case_id = fixture["caseId"].as_str().unwrap();
        let setup = parse_setup(&fixture["input"]["battleSetup"]);
        let mut runner = BattleRunner::new(&setup, &catalog).unwrap();

        for step in expand_steps(&fixture["input"]["steps"]) {
            let requests: Vec<ActionRequest> =
                serde_json::from_value(step["actionRequests"].clone()).unwrap();
            let shield_decisions: Vec<ShieldDecisionRequest> =
                serde_json::from_value(step["shieldDecisions"].clone()).unwrap();
            runner
                .advance_turn_with_shield_decisions(&requests, &shield_decisions)
                .unwrap_or_else(|error| panic!("fixture {case_id}: {error}"));
            if runner.is_finished() {
                break;
            }
        }

        assert!(
            runner.is_finished(),
            "fixture {case_id}: action stream did not finish the battle"
        );
        let actual = serde_json::to_value(runner.result().unwrap()).unwrap();
        let expected = &fixture["expectedResult"];
        if let Some(difference) = first_difference(expected, &actual, "$".to_owned()) {
            panic!("fixture {case_id} differs at {difference}");
        }
    }
}

fn parse_setup(value: &Value) -> BattleSetup {
    let participants: Vec<ParticipantSetup> = value["participants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|participant| {
            let team = participant["team"]
                .as_array()
                .unwrap()
                .iter()
                .map(|pokemon| {
                    let mut setup = PokemonSetup::new(
                        pokemon["pokemonId"].as_str().unwrap(),
                        serde_json::from_value::<PokemonBuildInput>(pokemon["build"].clone())
                            .unwrap(),
                        pokemon["fastMove"].as_str().unwrap(),
                        serde_json::from_value(pokemon["chargedMoves"].clone()).unwrap(),
                    );
                    if let Some(starting_hp) = pokemon["startingHp"].as_u64() {
                        setup = setup.with_starting_hp(starting_hp as u32);
                    }
                    if let Some(starting_energy) = pokemon["startingEnergy"].as_u64() {
                        setup = setup.with_starting_energy(starting_energy as u32);
                    }
                    setup
                })
                .collect();
            ParticipantSetup {
                participant_id: participant["participantId"].as_str().unwrap().to_owned(),
                controller_type: serde_json::from_value::<ControllerType>(
                    participant["controllerType"].clone(),
                )
                .unwrap(),
                shields_remaining: participant["shieldsRemaining"].as_u64().unwrap() as u8,
                team,
            }
        })
        .collect();

    BattleSetup {
        battle_id: value["battleId"].as_str().unwrap().to_owned(),
        rng_seed: value["rngSeed"].as_u64().unwrap(),
        participants: participants
            .try_into()
            .unwrap_or_else(|participants: Vec<_>| {
                panic!("expected two participants, got {}", participants.len())
            }),
    }
}

fn expand_steps(templates: &Value) -> Vec<Value> {
    templates
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|template| {
            let repeat_count = template["repeatCount"].as_u64().unwrap_or(1) as usize;
            (0..repeat_count).map(move |offset| {
                let mut step = template.clone();
                let step_object = step.as_object_mut().unwrap();
                step_object.remove("repeatCount");
                for request in step_object["actionRequests"].as_array_mut().unwrap() {
                    let turn = request["turn"].as_u64().unwrap() + offset as u64;
                    request["turn"] = Value::from(turn);
                }
                for decision in step_object
                    .entry("shieldDecisions")
                    .or_insert_with(|| Value::Array(Vec::new()))
                    .as_array_mut()
                    .unwrap()
                {
                    let turn = decision["turn"].as_u64().unwrap() + offset as u64;
                    decision["turn"] = Value::from(turn);
                }
                step
            })
        })
        .collect()
}

fn first_difference(expected: &Value, actual: &Value, path: String) -> Option<String> {
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            for key in expected.keys().chain(actual.keys()) {
                let child_path = format!("{path}.{key}");
                match (expected.get(key), actual.get(key)) {
                    (Some(expected), Some(actual)) => {
                        if let Some(difference) = first_difference(expected, actual, child_path) {
                            return Some(difference);
                        }
                    }
                    (Some(expected), None) => {
                        return Some(format!(
                            "{child_path}: missing from actual result (expected {expected})"
                        ));
                    }
                    (None, Some(actual)) => {
                        return Some(format!(
                            "{child_path}: unexpected in actual result ({actual})"
                        ));
                    }
                    (None, None) => unreachable!(),
                }
            }
            None
        }
        (Value::Array(expected), Value::Array(actual)) => {
            for index in 0..expected.len().min(actual.len()) {
                if let Some(difference) =
                    first_difference(&expected[index], &actual[index], format!("{path}[{index}]"))
                {
                    return Some(difference);
                }
            }
            if expected.len() == actual.len() {
                None
            } else {
                let first_extra = expected
                    .get(actual.len())
                    .map(|value| format!("; expected-only item: {value}"))
                    .or_else(|| {
                        actual
                            .get(expected.len())
                            .map(|value| format!("; actual-only item: {value}"))
                    })
                    .unwrap_or_default();
                Some(format!(
                    "{path}: expected array length {}, got {}{first_extra}",
                    expected.len(),
                    actual.len()
                ))
            }
        }
        _ if expected != actual => Some(format!("{path}: expected {expected}, got {actual}")),
        _ => None,
    }
}
