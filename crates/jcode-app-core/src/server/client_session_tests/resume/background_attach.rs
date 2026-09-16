/// Attaching to a background session must bring it back to the foreground:
/// the in-memory background tracker entry and the cross-process
/// `~/.jcode/background_pids` marker are both cleared, so the board stops
/// showing the session as "background running" the moment it is opened.
#[tokio::test]
async fn handle_resume_session_clears_background_registration() -> Result<()> {
    let _guard = crate::storage::lock_test_env();
    let (_runtime, prev_runtime) = setup_runtime_dir()?;
    let temp = tempfile::tempdir().map_err(|e| anyhow!(e))?;
    let prev_home = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", temp.path());

    let target_session_id = "session_background_target";
    let temp_session_id = "session_temp_foreground";

    let provider: Arc<dyn Provider> = Arc::new(MockProvider);
    let registry = Registry::new(provider.clone()).await;
    let target_agent = Arc::new(Mutex::new(build_test_agent_with_id(
        provider.clone(),
        registry.clone(),
        target_session_id,
        Vec::new(),
    )));
    let temp_agent = Arc::new(Mutex::new(build_test_agent_with_id(
        provider.clone(),
        registry.clone(),
        temp_session_id,
        Vec::new(),
    )));

    let sessions = Arc::new(RwLock::new(HashMap::from([
        (target_session_id.to_string(), Arc::clone(&target_agent)),
        (temp_session_id.to_string(), Arc::clone(&temp_agent)),
    ])));

    // The server kept the target alive after its client switched away mid-turn:
    // register it exactly like the session-switch path does.
    crate::server::background_session::register_background_session(
        target_session_id,
        Some("background target".to_string()),
    );
    assert!(crate::server::background_session::is_background_session(
        target_session_id
    ));
    assert!(crate::storage::session_is_background(target_session_id));

    let shutdown_signals = Arc::new(RwLock::new(HashMap::<String, InterruptSignal>::new()));
    let soft_interrupt_queues: SessionInterruptQueues = Arc::new(RwLock::new(HashMap::new()));
    let now = Instant::now();
    let client_connections = Arc::new(RwLock::new(HashMap::from([(
        "conn_new".to_string(),
        ClientConnectionInfo {
            client_id: "conn_new".to_string(),
            session_id: temp_session_id.to_string(),
            client_instance_id: None,
            debug_client_id: None,
            connected_at: now,
            last_seen: now,
            is_processing: false,
            current_tool_name: None,
            terminal_env: Vec::new(),
            disconnect_tx: mpsc::unbounded_channel().0,
        },
    )])));
    let swarm_members = Arc::new(RwLock::new(HashMap::<String, SwarmMember>::new()));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::<String, HashSet<String>>::new()));
    let file_touch = FileTouchService::new();
    let channel_subscriptions =
        Arc::new(RwLock::new(HashMap::<String, HashMap<String, HashSet<String>>>::new()));
    let channel_subscriptions_by_session =
        Arc::new(RwLock::new(HashMap::<String, HashMap<String, HashSet<String>>>::new()));
    let swarm_plans = Arc::new(RwLock::new(HashMap::<String, VersionedPlan>::new()));
    let swarm_coordinators = Arc::new(RwLock::new(HashMap::<String, String>::new()));
    let client_count = Arc::new(RwLock::new(1usize));
    let (writer, _peer_stream) = test_writer()?;
    let (client_event_tx, mut client_event_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let event_history = Arc::new(RwLock::new(VecDeque::<SwarmEvent>::new()));
    let event_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (swarm_event_tx, _swarm_event_rx) = broadcast::channel::<SwarmEvent>(8);
    let mcp_pool = Arc::new(crate::mcp::SharedMcpPool::from_default_config());

    let mut client_selfdev = false;
    let mut client_session_id = temp_session_id.to_string();

    handle_resume_session(
        91,
        target_session_id.to_string(),
        None,
        None,
        false,
        false,
        &mut client_selfdev,
        &mut client_session_id,
        "conn_new",
        &temp_agent,
        &provider,
        &registry,
        &sessions,
        &shutdown_signals,
        &soft_interrupt_queues,
        &client_connections,
        &Arc::new(RwLock::new(ClientDebugState::default())),
        &swarm_members,
        &swarms_by_id,
        &file_touch,
        &channel_subscriptions,
        &channel_subscriptions_by_session,
        &swarm_plans,
        &swarm_coordinators,
        &client_count,
        &writer,
        "test-server",
        "🌿",
        &client_event_tx,
        &mcp_pool,
        &event_history,
        &event_counter,
        &swarm_event_tx,
    )
    .await?;

    // Attaching brought the session back to the foreground: both the tracker
    // entry and the cross-process marker are gone.
    assert!(
        !crate::server::background_session::is_background_session(target_session_id),
        "resume attach must unregister the background session"
    );
    assert!(
        !crate::storage::session_is_background(target_session_id),
        "resume attach must clear the cross-process background marker"
    );

    let _ = collect_events_until_done(&mut client_event_rx, 91).await;

    match prev_home {
        Some(prev) => crate::env::set_var("JCODE_HOME", prev),
        None => crate::env::remove_var("JCODE_HOME"),
    }
    restore_runtime_dir(prev_runtime);
    Ok(())
}
