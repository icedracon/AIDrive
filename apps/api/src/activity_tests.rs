#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private PostgreSQL instance"]
async fn activity_flow_against_live_database() {
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("AIDRIVE_LIVE_TEST_URL").unwrap())
        .await
        .unwrap();
    let state = AppState {
        db: Some(db.clone()),
        web_origin: "http://localhost:3000".into(),
        allow_signup: true,
    };
    let mut cookies = Vec::new();
    for email in ["activity-owner@live.test", "activity-outsider@live.test"] {
        let creds = format!(r#"{{"email":"{email}","password":"correct horse battery staple"}}"#);
        let (_, _, cookie) = call(&state, "POST", "/auth/register", None, Some(&creds)).await;
        cookies.push(cookie.unwrap());
    }
    let (_, body, _) = call(&state, "POST", "/workspaces", Some(&cookies[0]), Some(r#"{"name":"Activity","review":"strict"}"#)).await;
    let ws = json_field(&body, "id");
    let mut tokens = Vec::new();
    for (n, name, scope) in [(61, "Codex", "files:write"), (62, "Meta", "workspace:read")] {
        let payload = format!(r#"{{"request_id":"{}","name":"{name}","scope":"{scope}"}}"#, request_uuid(n));
        let (_, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/agents"), Some(&cookies[0]), Some(&payload)).await;
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        tokens.push(body["token"].as_str().unwrap().to_owned());
    }
    let (status, _) = agent_send(
        &state,
        "POST",
        &format!("/agent/workspaces/{ws}/files"),
        &tokens[0],
        Some(format!(r#"{{"request_id":"{}","name":"notes.md","content":"hello"}}"#, request_uuid(63))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = call(&state, "GET", &format!("/workspaces/{ws}/activity"), Some(&cookies[0]), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let timeline: serde_json::Value = serde_json::from_str(&body).unwrap();
    let first = &timeline["events"][0];
    assert_eq!(first["kind"], "file.create");
    assert_eq!(first["agent_name"], "Codex");
    assert_eq!(first["actor_email"], "activity-owner@live.test");
    assert_eq!(first["subject"], "notes.md");
    assert!(timeline["events"].as_array().unwrap().iter().any(|e| e["kind"] == "agent.issued"));

    let (status, agent_view) = agent_send(&state, "GET", &format!("/agent/workspaces/{ws}/activity"), &tokens[0], None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(agent_view["events"][0]["kind"], "file.create", "agents can see what other assistants did");
    let (status, _) = agent_send(&state, "GET", &format!("/agent/workspaces/{ws}/activity"), &tokens[1], None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "workspace:read keys cannot read activity");

    let (status, _, _) = call(&state, "GET", &format!("/workspaces/{ws}/activity"), Some(&cookies[1]), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "outsiders cannot read activity");
    let (status, _, _) = call(&state, "GET", &format!("/workspaces/{ws}/activity?before=yesterday"), Some(&cookies[0]), None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, body, _) = call(&state, "GET", &format!("/workspaces/{ws}/activity?limit=1"), Some(&cookies[0]), None).await;
    assert_eq!(status, StatusCode::OK);
    let page: serde_json::Value = serde_json::from_str(&body).unwrap();
    let cursor = page["next_before"].as_str().expect("full page returns a cursor").to_owned();
    let (_, body, _) = call(&state, "GET", &format!("/workspaces/{ws}/activity?limit=50&before={cursor}"), Some(&cookies[0]), None).await;
    let rest: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(rest["events"].as_array().unwrap().iter().all(|e| e["kind"] != "file.create"));
    println!("PASS activity: owner and files:read agents see attributed events newest first, metadata keys and outsiders denied, cursor paging");
}
