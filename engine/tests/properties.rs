//! Property tests for the battle rules engine.
//!
//! The generated cases exercise the public API only: `BattleRunner`, the
//! snapshot/replay contracts, and the documented battle invariants. Each case
//! varies the seed, team size, builds, starting energy, shields, and a stream of
//! actions (fast move, charged move, swap, or wait) with matching shield
//! decisions.

use std::collections::HashSet;

use proptest::prelude::*;

use engine::battle::{BattleRunner, BattleSetup, ParticipantSetup, PokemonSetup};
use engine::data::DataCatalog;
use engine::model::{
    ActionRequest, BattleOutcome, BattleSnapshot, ControllerType, IndividualValues, PlayerAction,
    PokemonBuildInput, ShieldDecision, ShieldDecisionRequest, StatStages,
};
use engine::replay::{BATTLE_REPLAY_SCHEMA_VERSION, BattleReplay, ReplayRecorder};

const CATALOG_REVISION: &str = "embedded-proptest";
const TURN_DURATION_MS: u64 = 500;
/// A battle cannot bank energy past 100; `startingEnergy` is clamped to the
/// same value during setup.
const MAX_ENERGY: u32 = 100;
/// A hard bound for the finishing test; a battle that cannot decide in this
/// many turns is a bug, not a slow fixture.
const MAX_TURNS: usize = 1_000;

/// A small roster whose species, fast moves, and charged moves all exist in the
/// embedded gamemaster. Team size is generated, so teams of one and two are
/// both covered.
struct RosterEntry {
    species_id: &'static str,
    fast_move: &'static str,
    charged_moves: [&'static str; 2],
}

const ROSTER: [RosterEntry; 4] = [
    RosterEntry {
        species_id: "lanturn",
        fast_move: "WATER_GUN",
        charged_moves: ["SURF", "THUNDERBOLT"],
    },
    RosterEntry {
        species_id: "azumarill",
        fast_move: "BUBBLE",
        charged_moves: ["ICE_BEAM", "PLAY_ROUGH"],
    },
    RosterEntry {
        species_id: "pidgey",
        fast_move: "TACKLE",
        charged_moves: ["AERIAL_ACE", "TWISTER"],
    },
    RosterEntry {
        species_id: "skarmory",
        fast_move: "AIR_SLASH",
        charged_moves: ["SKY_ATTACK", "BRAVE_BIRD"],
    },
];

#[derive(Clone, Debug)]
struct BattlePlan {
    rng_seed: u64,
    left_team_len: usize,
    right_team_len: usize,
    builds: [(f32, IndividualValues); 4],
    shields: [u8; 2],
    starting_energy: [u32; 4],
    turns: Vec<PlannedTurn>,
}

#[derive(Clone, Debug)]
struct PlannedTurn {
    left: PlannedAction,
    right: PlannedAction,
    left_shield: Option<ShieldDecision>,
    right_shield: Option<ShieldDecision>,
}

#[derive(Clone, Copy, Debug)]
enum PlannedAction {
    Fast,
    Charged(usize),
    Swap(usize),
    Wait,
}

fn level_and_ivs() -> impl Strategy<Value = (f32, IndividualValues)> {
    (0u32..=80, 0u8..=15, 0u8..=15, 0u8..=15).prop_map(|(steps, attack, defense, hp)| {
        (
            1.0 + steps as f32 * 0.5,
            IndividualValues {
                attack,
                defense,
                hp,
            },
        )
    })
}

fn planned_action() -> impl Strategy<Value = PlannedAction> {
    prop_oneof![
        3 => Just(PlannedAction::Fast),
        2 => (0usize..3).prop_map(PlannedAction::Charged),
        1 => (0usize..3).prop_map(PlannedAction::Swap),
        1 => Just(PlannedAction::Wait),
    ]
}

fn shield_decision() -> impl Strategy<Value = ShieldDecision> {
    prop_oneof![Just(ShieldDecision::Shield), Just(ShieldDecision::NoShield),]
}

fn planned_turn() -> impl Strategy<Value = PlannedTurn> {
    (
        planned_action(),
        planned_action(),
        prop::option::of(shield_decision()),
        prop::option::of(shield_decision()),
    )
        .prop_map(|(left, right, left_shield, right_shield)| PlannedTurn {
            left,
            right,
            left_shield,
            right_shield,
        })
}

fn battle_plan() -> impl Strategy<Value = BattlePlan> {
    (
        any::<u64>(),
        1usize..=2,
        1usize..=2,
        (
            level_and_ivs(),
            level_and_ivs(),
            level_and_ivs(),
            level_and_ivs(),
        ),
        (0u8..=2, 0u8..=2),
        (
            0u32..=MAX_ENERGY,
            0u32..=MAX_ENERGY,
            0u32..=MAX_ENERGY,
            0u32..=MAX_ENERGY,
        ),
        prop::collection::vec(planned_turn(), 0..=24),
    )
        .prop_map(
            |(rng_seed, left_team_len, right_team_len, builds, shields, energy, turns)| {
                BattlePlan {
                    rng_seed,
                    left_team_len,
                    right_team_len,
                    builds: [builds.0, builds.1, builds.2, builds.3],
                    shields: [shields.0, shields.1],
                    starting_energy: [energy.0, energy.1, energy.2, energy.3],
                    turns,
                }
            },
        )
}

struct BattleRun {
    snapshots: Vec<BattleSnapshot>,
    error: Option<String>,
    replay: BattleReplay,
}

fn build_setup(plan: &BattlePlan) -> BattleSetup {
    BattleSetup {
        battle_id: "proptest-battle".to_owned(),
        rng_seed: plan.rng_seed,
        participants: [
            build_participant("left", &ROSTER[..plan.left_team_len], 0, plan),
            build_participant("right", &ROSTER[2..][..plan.right_team_len], 1, plan),
        ],
    }
}

fn build_participant(
    participant_id: &str,
    roster: &[RosterEntry],
    participant_index: usize,
    plan: &BattlePlan,
) -> ParticipantSetup {
    let team = roster
        .iter()
        .enumerate()
        .map(|(slot, entry)| {
            let build_index = participant_index * 2 + slot;
            let (level, ivs) = plan.builds[build_index].clone();
            PokemonSetup::new(
                format!("{participant_id}-{slot}"),
                PokemonBuildInput {
                    species_id: entry.species_id.to_owned(),
                    level,
                    ivs,
                    shadow: false,
                },
                entry.fast_move,
                entry
                    .charged_moves
                    .iter()
                    .map(|move_id| (*move_id).to_owned())
                    .collect(),
            )
            .with_starting_energy(plan.starting_energy[build_index])
        })
        .collect();

    ParticipantSetup {
        participant_id: participant_id.to_owned(),
        controller_type: ControllerType::Scripted,
        shields_remaining: plan.shields[participant_index],
        team,
    }
}

fn action_request(
    snapshot: &BattleSnapshot,
    participant_index: usize,
    planned: PlannedAction,
) -> ActionRequest {
    let participant = &snapshot.participants()[participant_index];
    let active = &participant.team[participant.active_pokemon_index];
    let action = match planned {
        PlannedAction::Fast => PlayerAction::FastMove {
            move_id: active.move_state.fast_move.clone(),
        },
        PlannedAction::Charged(index) => match active.move_state.charged_moves.as_slice() {
            [] => PlayerAction::Wait,
            choices => PlayerAction::ChargedMove {
                move_id: choices[index % choices.len()].clone(),
            },
        },
        PlannedAction::Swap(index) => PlayerAction::Swap {
            target_slot: index % participant.team.len(),
        },
        PlannedAction::Wait => PlayerAction::Wait,
    };

    ActionRequest {
        participant_id: participant.participant_id.clone(),
        turn: snapshot.turn(),
        action,
    }
}

fn shield_requests(snapshot: &BattleSnapshot, planned: &PlannedTurn) -> Vec<ShieldDecisionRequest> {
    [planned.left_shield.clone(), planned.right_shield.clone()]
        .into_iter()
        .enumerate()
        .filter_map(|(index, decision)| {
            decision.map(|decision| ShieldDecisionRequest {
                participant_id: snapshot.participants()[index].participant_id.clone(),
                turn: snapshot.turn(),
                decision,
            })
        })
        .collect()
}

fn run_plan(plan: &BattlePlan, catalog: &DataCatalog) -> BattleRun {
    let setup = build_setup(plan);
    let mut runner = BattleRunner::new(&setup, catalog).expect("generated setup must be valid");
    let mut recorder = ReplayRecorder::new(runner.snapshot().clone(), CATALOG_REVISION)
        .expect("catalog revision must be non-empty");
    let mut snapshots = vec![runner.snapshot().clone()];
    let mut error = None;

    assert_initial_invariants(runner.snapshot(), plan);

    for planned in &plan.turns {
        if runner.is_finished() {
            break;
        }

        let before = runner.snapshot().clone();
        let requests = [
            action_request(&before, 0, planned.left),
            action_request(&before, 1, planned.right),
        ];
        let shields = shield_requests(&before, planned);

        match runner.advance_turn_with_observer(&requests, &shields, &mut recorder) {
            Ok(_) => {
                assert_transition_invariants(plan, &before, runner.snapshot(), catalog);
                snapshots.push(runner.snapshot().clone());
            }
            Err(transition_error) => {
                error = Some(transition_error.to_string());
                break;
            }
        }
    }

    BattleRun {
        snapshots,
        error,
        replay: recorder.finish(),
    }
}

fn assert_initial_invariants(snapshot: &BattleSnapshot, plan: &BattlePlan) {
    assert_eq!(snapshot.turn(), 0);
    assert!(snapshot.timeline().is_empty());
    assert_eq!(snapshot.participants().len(), 2);

    for (participant_index, participant) in snapshot.participants().iter().enumerate() {
        assert_eq!(
            participant.shields_remaining,
            plan.shields[participant_index]
        );
        for pokemon in &participant.team {
            assert!(
                pokemon.energy <= MAX_ENERGY,
                "starting energy must be clamped to the cap"
            );
            assert!(pokemon.current_hp <= pokemon.max_hp);
        }
    }
}

/// Assert the rules that must hold for every resolved turn, independent of any
/// particular action stream.
fn assert_transition_invariants(
    plan: &BattlePlan,
    before: &BattleSnapshot,
    after: &BattleSnapshot,
    catalog: &DataCatalog,
) {
    let forced_swap_turn = !before.pending_forced_swaps().is_empty();
    if forced_swap_turn {
        assert_eq!(
            after.turn(),
            before.turn(),
            "resolving a forced swap must not advance the turn"
        );
    } else {
        assert_eq!(
            after.turn(),
            before.turn() + 1,
            "a resolved turn must advance exactly one turn"
        );
        assert_eq!(
            after.timeline().last().map(|record| record.turn),
            Some(before.turn()),
            "the new timeline record must describe the resolved turn"
        );
    }

    assert_eq!(
        after.timeline().len(),
        after.turn() as usize,
        "the timeline must carry exactly one record per turn"
    );
    assert_eq!(after.battle_id(), before.battle_id());
    assert_eq!(after.participants().len(), 2);

    let max_gain = max_fast_move_energy_gain(catalog);
    for (participant_index, (before_participant, after_participant)) in before
        .participants()
        .iter()
        .zip(after.participants())
        .enumerate()
    {
        assert_eq!(
            before_participant.participant_id,
            after_participant.participant_id
        );
        assert_eq!(
            before_participant.team.len(),
            after_participant.team.len(),
            "team membership must not change during a battle"
        );
        assert!(
            after_participant.shields_remaining <= before_participant.shields_remaining,
            "shields must never be restored"
        );
        assert!(
            after_participant.shields_remaining <= plan.shields[participant_index],
            "shields must not exceed the setup"
        );

        let active = &after_participant.team[after_participant.active_pokemon_index];
        let active_is_living = active.current_hp > 0 && !active.flags.fainted;
        assert_eq!(
            active.is_active, active_is_living,
            "the active flag must track whether the active Pokemon is alive"
        );

        for (slot, (before_pokemon, after_pokemon)) in before_participant
            .team
            .iter()
            .zip(&after_participant.team)
            .enumerate()
        {
            assert!(
                after_pokemon.current_hp <= after_pokemon.max_hp,
                "HP must never exceed max HP"
            );
            assert_eq!(
                after_pokemon.flags.fainted,
                after_pokemon.current_hp == 0,
                "the fainted flag must match zero HP"
            );
            // PvPoke caps energy at 100; gains also never exceed one fast move.
            assert!(
                after_pokemon.energy <= MAX_ENERGY,
                "energy must never exceed the 100-point cap"
            );
            assert!(
                after_pokemon.energy <= before_pokemon.energy + max_gain,
                "energy may only grow by the completed fast-move gain"
            );
            assert!(
                after_pokemon.pending_energy_gain <= max_gain,
                "pending energy must come from a single fast move"
            );
            assert!(
                (StatStages::MIN..=StatStages::MAX)
                    .contains(&after_pokemon.stat_stages.attack_stage())
                    && (StatStages::MIN..=StatStages::MAX)
                        .contains(&after_pokemon.stat_stages.defense_stage()),
                "stat stages must stay clamped to four stages"
            );
            if slot != after_participant.active_pokemon_index {
                assert!(
                    !after_pokemon.is_active,
                    "only one Pokemon per participant may be active"
                );
            }
        }
    }

    assert_pending_charged_moves(after);
    assert_forced_swaps(after);
}

fn assert_pending_charged_moves(snapshot: &BattleSnapshot) {
    let mut seen = HashSet::new();
    for pending in snapshot.pending_charged_moves() {
        assert!(
            seen.insert(pending.participant_id.as_str()),
            "a participant may only have one queued charged move"
        );

        let participant = snapshot
            .participants()
            .iter()
            .find(|participant| participant.participant_id == pending.participant_id)
            .expect("a queued charged move must belong to a participant");
        let active = &participant.team[participant.active_pokemon_index];

        assert!(
            active.current_hp > 0 && !active.flags.fainted,
            "a queued charged move must belong to a living Pokemon"
        );
        assert!(
            active.move_state.charged_moves.contains(&pending.move_id),
            "a queued charged move must be one of the active Pokemon's moves"
        );
    }
}

fn assert_forced_swaps(snapshot: &BattleSnapshot) {
    let queued: HashSet<&str> = snapshot
        .pending_forced_swaps()
        .iter()
        .map(|pending| pending.participant_id.as_str())
        .collect();

    for pending in snapshot.pending_forced_swaps() {
        let participant = snapshot
            .participants()
            .iter()
            .find(|participant| participant.participant_id == pending.participant_id)
            .expect("a forced swap must belong to a participant");
        let active = &participant.team[participant.active_pokemon_index];

        assert!(
            active.current_hp == 0 || active.flags.fainted,
            "a forced swap implies a fainted active Pokemon"
        );
        assert!(
            !pending.options.is_empty(),
            "a forced swap must offer at least one replacement"
        );
        for option in &pending.options {
            assert!(
                *option != participant.active_pokemon_index,
                "a forced swap must replace the fainted active Pokemon"
            );
            let replacement = participant
                .team
                .iter()
                .find(|pokemon| pokemon.slot == *option)
                .expect("a forced swap option must exist on the team");
            assert!(
                replacement.current_hp > 0 && !replacement.flags.fainted,
                "a forced swap option must be alive"
            );
        }
    }

    for participant in snapshot.participants() {
        let active = &participant.team[participant.active_pokemon_index];
        let active_is_fainted = active.current_hp == 0 || active.flags.fainted;
        let has_living_bench = participant
            .team
            .iter()
            .any(|pokemon| pokemon.current_hp > 0 && !pokemon.flags.fainted);
        if active_is_fainted && has_living_bench {
            assert!(
                queued.contains(participant.participant_id.as_str()),
                "a fainted active Pokemon with a living bench must queue a forced swap"
            );
        }
    }
}

fn max_fast_move_energy_gain(catalog: &DataCatalog) -> u32 {
    ROSTER
        .iter()
        .filter_map(|entry| catalog.move_definition(entry.fast_move))
        .map(|move_definition| move_definition.energy_gain)
        .max()
        .expect("the roster must reference known fast moves")
}

fn decisive_requests(snapshot: &BattleSnapshot, catalog: &DataCatalog) -> Vec<ActionRequest> {
    snapshot
        .participants()
        .iter()
        .map(|participant| {
            let forced_swap = snapshot
                .pending_forced_swaps()
                .iter()
                .find(|pending| pending.participant_id == participant.participant_id);
            let action = if let Some(forced_swap) = forced_swap {
                PlayerAction::Swap {
                    target_slot: forced_swap.options[0],
                }
            } else {
                let active = &participant.team[participant.active_pokemon_index];
                let affordable = active.move_state.charged_moves.iter().find(|move_id| {
                    catalog
                        .charged_move_cost(move_id)
                        .is_some_and(|cost| active.energy >= cost)
                });
                affordable.map_or_else(
                    || PlayerAction::FastMove {
                        move_id: active.move_state.fast_move.clone(),
                    },
                    |move_id| PlayerAction::ChargedMove {
                        move_id: move_id.clone(),
                    },
                )
            };

            ActionRequest {
                participant_id: participant.participant_id.clone(),
                turn: snapshot.turn(),
                action,
            }
        })
        .collect()
}

fn decisive_shields(snapshot: &BattleSnapshot) -> Vec<ShieldDecisionRequest> {
    snapshot
        .participants()
        .iter()
        .map(|participant| ShieldDecisionRequest {
            participant_id: participant.participant_id.clone(),
            turn: snapshot.turn(),
            decision: ShieldDecision::Shield,
        })
        .collect()
}

fn round_trip(snapshot: &BattleSnapshot) -> BattleSnapshot {
    let json = serde_json::to_string(snapshot).expect("a snapshot must serialize");
    serde_json::from_str(&json).expect("a serialized snapshot must deserialize")
}

proptest! {
    #[test]
    fn battles_are_deterministic_replayable_and_state_bounded(plan in battle_plan()) {
        let catalog = DataCatalog::embedded().expect("the embedded gamemaster must load");

        let first = run_plan(&plan, &catalog);
        let second = run_plan(&plan, &catalog);

        prop_assert_eq!(&first.error, &second.error);
        prop_assert_eq!(&first.snapshots, &second.snapshots);

        for snapshot in &second.snapshots {
            prop_assert_eq!(&round_trip(snapshot), snapshot);
        }

        let serialized = serde_json::to_string(&second.replay).expect("a replay must serialize");
        let replay: BattleReplay =
            serde_json::from_str(&serialized).expect("a serialized replay must deserialize");
        prop_assert_eq!(&replay, &second.replay);
        prop_assert_eq!(replay.schema_version, BATTLE_REPLAY_SCHEMA_VERSION);

        let replayed = replay.replay(&catalog, CATALOG_REVISION);
        match replayed {
            Ok(snapshot) => prop_assert_eq!(
                &snapshot,
                second.snapshots.last().expect("a run always records its initial snapshot")
            ),
            Err(error) => prop_assert!(false, "the recorded replay must replay cleanly: {error}"),
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn battles_finish_and_report_a_consistent_result(plan in battle_plan()) {
        let catalog = DataCatalog::embedded().expect("the embedded gamemaster must load");
        let setup = build_setup(&plan);
        let mut runner = BattleRunner::new(&setup, &catalog).expect("generated setup must be valid");
        let mut turns = 0usize;

        while !runner.is_finished() {
            prop_assert!(turns < MAX_TURNS, "the battle must decide within {MAX_TURNS} turns");

            let requests = decisive_requests(runner.snapshot(), &catalog);
            let shields = decisive_shields(runner.snapshot());
            runner
                .advance_turn_with_shield_decisions(&requests, &shields)
                .expect("the decisive policy must always produce a legal turn");
            turns += 1;
        }

        let result = runner.result().expect("a finished battle must produce a result");
        prop_assert_eq!(result.total_turns, runner.snapshot().turn());
        prop_assert_eq!(
            result.battle_duration_ms,
            u64::from(result.total_turns) * TURN_DURATION_MS
        );
        prop_assert_eq!(result.participants.len(), 2);

        for participant in &result.participants {
            prop_assert!(participant.final_hp <= participant.max_hp);
            prop_assert!(participant.opponent_damage <= participant.opponent_max_hp);
            prop_assert!(participant.rating <= 1_000);
        }

        for (result_participant, participant) in
            result.participants.iter().zip(runner.snapshot().participants())
        {
            let final_hp: u32 = participant
                .team
                .iter()
                .map(|pokemon| {
                    if pokemon.flags.fainted {
                        0
                    } else {
                        pokemon.current_hp.min(pokemon.max_hp)
                    }
                })
                .sum();
            prop_assert_eq!(result_participant.final_hp, final_hp);
        }

        let living: Vec<bool> = runner
            .snapshot()
            .participants()
            .iter()
            .map(|participant| {
                participant
                    .team
                    .iter()
                    .any(|pokemon| pokemon.current_hp > 0 && !pokemon.flags.fainted)
            })
            .collect();

        match (living[0], living[1]) {
            (true, false) => {
                prop_assert!(matches!(&result.outcome, BattleOutcome::Participant0Win));
                prop_assert_eq!(
                    result.winner.as_deref(),
                    Some(result.participants[0].participant_id.as_str())
                );
                prop_assert_eq!(
                    result.loser.as_deref(),
                    Some(result.participants[1].participant_id.as_str())
                );
            }
            (false, true) => {
                prop_assert!(matches!(&result.outcome, BattleOutcome::Participant1Win));
                prop_assert_eq!(
                    result.winner.as_deref(),
                    Some(result.participants[1].participant_id.as_str())
                );
                prop_assert_eq!(
                    result.loser.as_deref(),
                    Some(result.participants[0].participant_id.as_str())
                );
            }
            (false, false) => {
                prop_assert!(matches!(&result.outcome, BattleOutcome::Draw));
                prop_assert!(result.winner.is_none());
                prop_assert!(result.loser.is_none());
            }
            (true, true) => prop_assert!(false, "a finished battle cannot have two living sides"),
        }
    }
}
