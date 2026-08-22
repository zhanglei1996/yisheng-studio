use super::*;

#[test]
fn saving_identical_project_tts_settings_preserves_ready_audio() {
    let db = Database::memory().unwrap();
    db.create_project("p1", "Course").unwrap();
    db.upsert_segment(&segment("s1", 0, 0, 1_000)).unwrap();
    db.set_project_tts_defaults(
        "p1",
        "aliyun",
        Some("Cherry"),
        "professional",
        "{}",
        true,
        "balanced",
    )
    .unwrap();
    db.set_segment_tts_state("s1", "ready", Some("hash-1"), Some(980), None)
        .unwrap();

    let unchanged = db
        .set_project_tts_defaults(
            "p1",
            "aliyun",
            Some("Cherry"),
            "professional",
            "{}",
            true,
            "balanced",
        )
        .unwrap();

    assert_eq!(unchanged.tts_settings_revision, 2);
    assert_eq!(db.get_segment("s1").unwrap().tts_state, "ready");
}

#[test]
fn one_tts_snapshot_can_checkpoint_multiple_completed_blocks() {
    let db = Database::memory().unwrap();
    db.create_project("p1", "Course").unwrap();
    db.upsert_segment(&segment("s1", 0, 0, 1_000)).unwrap();
    db.upsert_segment(&segment("s2", 1, 1_000, 2_000)).unwrap();
    running_tts_job(&db);
    let snapshot = db.capture_tts_publish_snapshot("p1", 1).unwrap();

    for id in ["s1", "s2"] {
        db.commit_tts_publication(
            "j-tts",
            &snapshot,
            &[TtsSegmentPublication {
                segment_id: id.into(),
                expected_script_revision: 1,
                state: "ready".into(),
                settings_hash: Some(format!("hash-{id}")),
                duration_ms: Some(900),
                error_message: None,
                display_status: "ready".into(),
            }],
            &[],
        )
        .unwrap();
    }

    assert_eq!(db.get_segment("s1").unwrap().tts_state, "ready");
    assert_eq!(db.get_segment("s2").unwrap().tts_state, "ready");
}
