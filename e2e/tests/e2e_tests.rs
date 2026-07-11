use friday_e2e::runner::run_vertical_slice;

#[tokio::test]
async fn test_vertical_slice_end_to_end() {
    let goal = "Create a folder named Research, initialize a Git repository, create a README.md, commit it, and open the folder.";
    
    let temp_dir = std::env::temp_dir().join(format!("friday-e2e-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let target_dir_str = temp_dir.to_string_lossy().to_string().replace("\\", "/");
    let mock_llm_json = r##"{
        "goal": "Create a folder named Research, initialize a Git repository, create a README.md, commit it, and open the folder.",
        "intent": "workspace.setup",
        "confidence": 0.98,
        "tasks": [
            {
                "name": "Create README.md file",
                "capability": "filesystem.create_file",
                "params": {
                    "path": "TARGET_DIR/Research/README.md",
                    "content": "# Research\nInitial repository."
                },
                "depends_on_indices": []
            },
            {
                "name": "Initialize Git repository",
                "capability": "git.init",
                "params": {
                    "path": "TARGET_DIR/Research"
                },
                "depends_on_indices": [0]
            },
            {
                "name": "Stage files for commit",
                "capability": "git.add",
                "params": {
                    "path": "TARGET_DIR/Research",
                    "paths": ["README.md"]
                },
                "depends_on_indices": [0, 1]
            },
            {
                "name": "Commit README to git",
                "capability": "git.commit",
                "params": {
                    "path": "TARGET_DIR/Research",
                    "message": "Initial commit"
                },
                "depends_on_indices": [2]
            },
            {
                "name": "Open directory in Explorer",
                "capability": "windows.open",
                "params": {
                    "app": "explorer",
                    "args": ["TARGET_DIR/Research"]
                },
                "depends_on_indices": [0]
            }
        ]
    }"##.replace("TARGET_DIR", &target_dir_str);

    let trace = run_vertical_slice(goal, &mock_llm_json, &temp_dir).await.unwrap();

    // Verify telemetry trace outcomes
    assert_eq!(trace.goal, goal);
    if !trace.execution_success {
        panic!("Execution failed. Errors: {:?}", trace.execution_errors);
    }
    assert_eq!(trace.task_durations_ms.len(), 5);
    assert!(trace.execution_errors.is_empty());
    
    // Verify trace saving and loading roundtrip
    let trace_file = temp_dir.join("trace.json");
    friday_e2e::trace::TraceReplayEngine::save(&trace, &trace_file).unwrap();
    
    let loaded = friday_e2e::trace::TraceReplayEngine::load(&trace_file).unwrap();
    assert_eq!(loaded.goal, goal);
    assert_eq!(loaded.task_durations_ms.len(), 5);

    let _ = std::fs::remove_dir_all(&temp_dir);
}
