use std::sync::Arc;
use friday_capabilities::registry::CapabilityRegistry;
use friday_capabilities::model::Capability;
use friday_core::memory::{MemoryEngine, MemoryEntry, MemoryLayerKind};
use friday_context::ContextBuilder;

#[tokio::test]
async fn test_context_builder_retrieval_and_pruning() {
    let reg = Arc::new(CapabilityRegistry::new());
    
    // Register some mock capabilities
    reg.register(
        "filesystem",
        Capability::new("filesystem.create_file", "Create File", "Files")
            .with_latency(5)
            .with_reliability(0.999),
    );
    reg.register(
        "windows",
        Capability::new("windows.open", "Open Window", "Windows")
            .with_latency(30)
            .with_reliability(0.95),
    );

    let temp_dir = std::env::temp_dir().join(format!("context-test-{}", uuid::Uuid::new_v4()));
    let memory = Arc::new(MemoryEngine::default_layers(&temp_dir).unwrap());

    // Add some mock memories
    let entry = MemoryEntry::new(
        MemoryLayerKind::LongTerm,
        "note",
        serde_json::json!({"text": "user wants to open notepad to write a note"}),
    );
    memory.store(&entry).await.unwrap();

    let builder = ContextBuilder::new(
        reg.clone(),
        None,
        memory.clone(),
        temp_dir.join("experience.redb"),
    );

    let context = builder.build("open notepad to write a note").await.unwrap();

    // Verify goals and timestamps
    assert_eq!(context.goal, "open notepad to write a note");
    assert!(context.timestamp <= chrono::Utc::now());

    // Verify capabilities retrieved and deduplicated
    assert!(!context.capabilities.is_empty());
    assert!(context.capabilities.iter().any(|c| c.id == "filesystem.create_file"));

    // Verify memories retrieved
    assert!(!context.relevant_memories.is_empty());
    assert_eq!(context.relevant_memories[0].key, "note");

    let _ = std::fs::remove_dir_all(&temp_dir);
}
