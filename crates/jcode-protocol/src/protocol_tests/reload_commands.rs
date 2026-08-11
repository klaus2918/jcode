// Roundtrip tests for the dynamic-reload wire messages: `ReloadMcp`,
// `ReloadSkills` requests and the `Skills` server event.

#[test]
fn test_reload_mcp_request_roundtrip() -> Result<()> {
    let req = Request::ReloadMcp { id: 42 };
    let json = serde_json::to_string(&req)?;
    assert!(json.contains("\"type\":\"reload_mcp\""));
    let decoded = parse_request_json(&json)?;
    assert_eq!(decoded.id(), 42);
    let Request::ReloadMcp { id } = decoded else {
        return Err(anyhow!("wrong request type"));
    };
    assert_eq!(id, 42);
    Ok(())
}

#[test]
fn test_reload_skills_request_roundtrip() -> Result<()> {
    let req = Request::ReloadSkills { id: 43 };
    let json = serde_json::to_string(&req)?;
    assert!(json.contains("\"type\":\"reload_skills\""));
    let decoded = parse_request_json(&json)?;
    assert_eq!(decoded.id(), 43);
    let Request::ReloadSkills { id } = decoded else {
        return Err(anyhow!("wrong request type"));
    };
    assert_eq!(id, 43);
    Ok(())
}

#[test]
fn test_skills_event_roundtrip() -> Result<()> {
    let event = ServerEvent::Skills {
        skills: vec!["optimization".to_string(), "docx".to_string()],
    };
    let json = serde_json::to_string(&event)?;
    assert!(json.contains("\"type\":\"skills\""));
    let decoded = parse_event_json(&json)?;
    let ServerEvent::Skills { skills } = decoded else {
        return Err(anyhow!("wrong event type"));
    };
    assert_eq!(
        skills,
        vec!["optimization".to_string(), "docx".to_string()]
    );
    Ok(())
}

#[test]
fn test_skills_event_decodes_empty_list() -> Result<()> {
    let json = r#"{"type":"skills","skills":[]}"#;
    let decoded = parse_event_json(json)?;
    let ServerEvent::Skills { skills } = decoded else {
        return Err(anyhow!("wrong event type"));
    };
    assert!(skills.is_empty());
    Ok(())
}
