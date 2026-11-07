use engine::model::BattleSnapshot;

#[test]
fn snapshot_can_be_constructed_and_read_through_the_public_api() {
    let snapshot = BattleSnapshot::new("public-api-test", 37, Vec::new())
        .with_turn(4)
        .with_metadata([("source".to_owned(), "integration-test".to_owned())].into());

    assert_eq!(snapshot.battle_id(), "public-api-test");
    assert_eq!(snapshot.turn(), 4);
    assert_eq!(snapshot.rng_state(), 37);
    assert!(snapshot.participants().is_empty());
    assert!(snapshot.pending_charged_moves().is_empty());
    assert!(snapshot.pending_forced_swaps().is_empty());
    assert!(snapshot.timeline().is_empty());
    assert_eq!(
        snapshot.metadata().get("source").unwrap(),
        "integration-test"
    );
}

#[test]
fn private_snapshot_fields_keep_the_existing_json_contract() {
    let snapshot = BattleSnapshot::new("serialization-test", 21, Vec::new());
    let json = serde_json::to_value(&snapshot).unwrap();

    assert_eq!(json["battleId"], "serialization-test");
    assert_eq!(json["turn"], 0);
    assert_eq!(json["rngState"], 21);
    assert_eq!(json["participants"], serde_json::json!([]));

    let restored: BattleSnapshot = serde_json::from_value(json).unwrap();
    assert_eq!(restored, snapshot);
}
