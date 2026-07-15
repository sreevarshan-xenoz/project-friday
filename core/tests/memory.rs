//! Integration test for the memory backends: every tier stores, retrieves,
//! and searches, and the durable tiers survive a reopen.

use friday_core::memory::{
    MemoryEngine, MemoryEntry, MemoryLayer, MemoryLayerKind, SessionMemory, VectorMemory,
};

#[tokio::test]
async fn working_session_vector_roundtrip() {
    friday_core::init_telemetry();

    // Working memory (ephemeral).
    let working = friday_core::WorkingMemory::new();
    let w_entry = MemoryEntry::new(
        MemoryLayerKind::Working,
        "note",
        serde_json::json!({"t": "hi"}),
    );
    working.store(&w_entry).await.unwrap();
    assert_eq!(
        working.retrieve("note").await.unwrap().unwrap().content,
        serde_json::json!({"t": "hi"})
    );

    // Session memory (durable redb) — stores, reopens, still there.
    let dir = std::env::temp_dir().join(format!("friday-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    {
        let session = SessionMemory::open(&dir.join("session.redb")).unwrap();
        let s_entry = MemoryEntry::new(
            MemoryLayerKind::Session,
            "cmd",
            serde_json::json!({"cmd": "ls"}),
        );
        session.store(&s_entry).await.unwrap();
        let found = session.retrieve("cmd").await.unwrap().unwrap();
        assert_eq!(found.content, serde_json::json!({"cmd": "ls"}));
        assert_eq!(found.layer, MemoryLayerKind::Session);
    }
    // Reopen and confirm durability.
    {
        let session = SessionMemory::open(&dir.join("session.redb")).unwrap();
        assert!(session.retrieve("cmd").await.unwrap().is_some());
    }
    let _ = std::fs::remove_dir_all(&dir);

    // Vector memory — semantic search by cosine similarity.
    let vector = VectorMemory::new();
    let mut a = MemoryEntry::new(
        MemoryLayerKind::Vector,
        "a",
        serde_json::json!({"k": "cat"}),
    );
    a.embedding = Some(vec![1.0, 0.0, 0.0]);
    let mut b = MemoryEntry::new(
        MemoryLayerKind::Vector,
        "b",
        serde_json::json!({"k": "dog"}),
    );
    b.embedding = Some(vec![0.0, 1.0, 0.0]);
    vector.store(&a).await.unwrap();
    vector.store(&b).await.unwrap();
    let top = vector.search_similar(&[0.9, 0.1, 0.0], 1);
    assert_eq!(top[0].key, "a");
}

#[tokio::test]
async fn engine_routes_by_layer() {
    let dir = std::env::temp_dir().join(format!("friday-engine-{}", uuid::Uuid::new_v4()));
    let engine = MemoryEngine::default_layers(&dir).unwrap();
    let entry = MemoryEntry::new(
        MemoryLayerKind::LongTerm,
        "fact",
        serde_json::json!({"v": 42}),
    );
    engine.store(&entry).await.unwrap();
    let got = engine
        .retrieve(MemoryLayerKind::LongTerm, "fact")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.content, serde_json::json!({"v": 42}));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_experience_engine_durability_and_queries() {
    use chrono::Utc;
    use friday_core::memory::experience::{ExecutionExperience, ExperienceEngine, ExperienceTask};
    use uuid::Uuid;

    let dir = std::env::temp_dir().join(format!("friday-experience-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("experience.redb");

    let exp_id = Uuid::new_v4();
    let plan_id = Uuid::new_v4();

    // 1. Open database and record an experience
    {
        let engine = ExperienceEngine::open(&db_path).unwrap();
        let exp = ExecutionExperience {
            experience_id: exp_id,
            intent: "build cargo project".to_string(),
            plan_id,
            success: true,
            actual_duration_ms: 1200,
            predicted_duration_ms: 1000,
            timing_drift: 0.2,
            tasks: vec![ExperienceTask {
                id: Uuid::new_v4(),
                name: "compile cargo".to_string(),
                capability: "terminal.run".to_string(),
                success: true,
                duration_ms: 1200,
                retry_count: 0,
                error: None,
            }],
            resource_forecast: serde_json::json!({"cpu": 10.0}),
            resource_actual: serde_json::json!({"cpu": 12.0}),
            timestamp: Utc::now(),
        };

        engine.record(&exp).unwrap();

        // Retrieve and check
        let retrieved = engine.retrieve(exp_id).unwrap().unwrap();
        assert_eq!(retrieved.intent, "build cargo project");
        assert_eq!(retrieved.actual_duration_ms, 1200);
        assert!(retrieved.success);
        assert_eq!(retrieved.tasks.len(), 1);
        assert_eq!(retrieved.tasks[0].capability, "terminal.run");
    }

    // 2. Reopen and check durability and queries
    {
        let engine = ExperienceEngine::open(&db_path).unwrap();
        let retrieved = engine.retrieve(exp_id).unwrap().unwrap();
        assert_eq!(retrieved.intent, "build cargo project");

        let all = engine.list().unwrap();
        assert_eq!(all.len(), 1);

        let matches = engine.query_by_intent("cargo").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].experience_id, exp_id);

        let none_matches = engine.query_by_intent("windows").unwrap();
        assert_eq!(none_matches.len(), 0);
    }

    let _ = std::fs::remove_dir_all(&dir);
}
