use std::any::Any;
use std::fs;
use std::panic::{self, AssertUnwindSafe};
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
    let mut report = String::new();
    let mut failure_count = 0;
    for fixture_path in &fixture_paths {
        let label = fixture_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("<unknown>");

        let outcome = panic::catch_unwind(AssertUnwindSafe(|| run_fixture(fixture_path, &catalog)));
        match outcome {
            Ok(Ok(case_id)) => {
                println!("PASS {case_id}");
                report.push_str(&format!("PASS {case_id}\n"));
            }
            Ok(Err(failure)) => {
                failure_count += 1;
                eprintln!("FAIL {failure}");
                report.push_str(&format!("FAIL {failure}\n"));
            }
            Err(payload) => {
                failure_count += 1;
                let failure = format!("{label}: {}", panic_message(payload));
                eprintln!("FAIL {failure}");
                report.push_str(&format!("FAIL {failure}\n"));
            }
        }
    }

    assert_eq!(
        failure_count,
        0,
        "{failure_count} of {} PvPoke fixture(s) failed:\n{report}",
        fixture_paths.len()
    );
}

fn run_fixture(fixture_path: &Path, catalog: &DataCatalog) -> Result<String, String> {
    let fixture: Value = serde_json::from_slice(
        &fs::read(fixture_path).map_err(|error| format!("{}: {error}", fixture_path.display()))?,
    )
    .map_err(|error| format!("{}: {error}", fixture_path.display()))?;

    let case_id = fixture["caseId"]
        .as_str()
        .ok_or_else(|| format!("{}: missing caseId", fixture_path.display()))?;
    if fixture["fixtureVersion"] != 1 {
        return Err(format!(
            "{case_id}: unsupported fixtureVersion {}",
            fixture["fixtureVersion"]
        ));
    }

    let setup = parse_setup(&fixture["input"]["battleSetup"]);
    let mut runner =
        BattleRunner::new(&setup, catalog).map_err(|error| format!("{case_id}: {error}"))?;

    for step in expand_steps(&fixture["input"]["steps"]) {
        let step_kind = step["stepKind"].as_str().unwrap_or("turn");
        let step_turn = step["actionRequests"][0]["turn"]
            .as_u64()
            .unwrap_or_default();
        let requests: Vec<ActionRequest> = serde_json::from_value(step["actionRequests"].clone())
            .map_err(|error| format!("{case_id}: {error}"))?;
        let shield_decisions: Vec<ShieldDecisionRequest> =
            serde_json::from_value(step["shieldDecisions"].clone())
                .map_err(|error| format!("{case_id}: {error}"))?;

        let pending = runner.snapshot().pending_forced_swaps();
        match step_kind {
            "forcedSwitch" => {
                if pending.is_empty() {
                    return Err(format!(
                        "{case_id}: forcedSwitch step for turn {step_turn} has no pending replacement choice"
                    ));
                }
            }
            "turn" => {
                if !pending.is_empty() {
                    let participants = pending
                        .iter()
                        .map(|pending| pending.participant_id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(format!(
                        "{case_id}: turn {step_turn} must be a forcedSwitch continuation; pending replacement choice for {participants}"
                    ));
                }
            }
            other => {
                return Err(format!(
                    "{case_id}: unsupported stepKind {other} on turn {step_turn}"
                ));
            }
        }

        let turn_before = runner.snapshot().turn();
        runner
            .advance_turn_with_shield_decisions(&requests, &shield_decisions)
            .map_err(|error| format!("{case_id}: {error}"))?;
        let turn_after = runner.snapshot().turn();

        if step_kind == "forcedSwitch" {
            if turn_after != turn_before {
                return Err(format!(
                    "{case_id}: forcedSwitch step for turn {step_turn} advanced the turn from {turn_before} to {turn_after}"
                ));
            }
            if !runner.snapshot().pending_forced_swaps().is_empty() {
                return Err(format!(
                    "{case_id}: forcedSwitch step for turn {step_turn} did not resolve every pending replacement choice"
                ));
            }
        } else if turn_after == turn_before {
            return Err(format!(
                "{case_id}: normal step for turn {step_turn} did not advance the turn"
            ));
        }

        if runner.is_finished() {
            break;
        }
    }

    if !runner.is_finished() {
        if !runner.snapshot().pending_forced_swaps().is_empty() {
            return Err(format!(
                "{case_id}: action stream ended with an unresolved pending replacement choice"
            ));
        }
        return Err(format!(
            "{case_id}: action stream did not finish the battle"
        ));
    }

    let actual = serde_json::to_value(runner.result().unwrap()).unwrap();
    let expected = &fixture["expectedResult"];
    if let Some(difference) = first_difference(expected, &actual, "$".to_owned()) {
        return Err(format!("{case_id} differs at {difference}"));
    }

    Ok(case_id.to_owned())
}

fn panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_owned();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "fixture panicked with a non-string payload".to_owned()
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
