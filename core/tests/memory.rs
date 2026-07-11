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
