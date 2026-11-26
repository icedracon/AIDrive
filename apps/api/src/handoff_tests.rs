#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private PostgreSQL instance"]
async fn handoff_flow_against_live_database() {
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
    let creds = r#"{"email":"handoff-owner@live.test","password":"correct horse battery staple"}"#;
    let (_, _, cookie) = call(&state, "POST", "/auth/register", None, Some(creds)).await;
    let cookie = cookie.unwrap();
    let (_, body, _) = call(&state, "POST", "/workspaces", Some(&cookie), Some(r#"{"name":"Handoff","review":"strict"}"#)).await;
    let ws = json_field(&body, "id");
    let mut tokens = Vec::new();
    for (n, scope) in [(31, "files:write"), (32, "files:write"), (33, "files:read")] {
        let payload = format!(
            r#"{{"request_id":"{}","name":"agent {n}","scope":"{scope}"}}"#,
            request_uuid(n)
        );
        let (_, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/agents"), Some(&cookie), Some(&payload)).await;
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        tokens.push(body["token"].as_str().unwrap().to_owned());
    }
    let (claude, codex, reader) = (&tokens[0], &tokens[1], &tokens[2]);
    let agent = format!("/agent/workspaces/{ws}");

    let (status, file) = agent_send(
        &state,
        "POST",
        &format!("{agent}/files"),
        claude,
        Some(format!(r#"{{"request_id":"{}","name":"brief.md","content":"v1"}}"#, request_uuid(40))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (file_id, v1) = (file["file_id"].as_str().unwrap().to_owned(), file["version_id"].as_str().unwrap().to_owned());

    let handoff = |n: u32, summary: &str, version: &str| {
        Some(format!(
            r#"{{"request_id":"{}","summary":"{summary}","decisions":["Audience: solo founders"],"next_step":"Codex drafts interview questions","sources":[{{"file_id":"{file_id}","version_id":"{version}"}}]}}"#,
            request_uuid(n)
        ))
    };
    let (status, h1) = agent_send(&state, "POST", &format!("{agent}/handoffs"), claude, handoff(41, "Drafted brief", &v1)).await;
    assert_eq!(status, StatusCode::CREATED, "{h1}");
    assert_eq!(h1["status"], "proposed");
    assert_eq!(h1["agent_id"], request_uuid(31));
    assert_eq!(h1["stale"], false);
    let h1_id = h1["id"].as_str().unwrap().to_owned();

    let (status, body) = agent_send(&state, "POST", &format!("{agent}/handoffs"), reader, handoff(42, "x", &v1)).await;
    assert_eq!((status, body["error"].as_str()), (StatusCode::FORBIDDEN, Some("insufficient_scope")));
    let (status, _) = agent_send(&state, "POST", &format!("{agent}/handoffs/{h1_id}/decision"), claude, Some(r#"{"decision":"approved"}"#.into())).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a low key cannot decide handoffs");

    let (status, latest) = agent_send(&state, "GET", &format!("{agent}/handoffs/latest"), codex, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(latest["approved"].is_null(), "nothing is approved before the owner decides");
    assert_eq!(latest["pending"][0]["id"], h1_id.as_str());

    let (status, decided, _) = call(
        &state,
        "POST",
        &format!("/workspaces/{ws}/handoffs/{h1_id}/decision"),
        Some(&cookie),
        Some(r#"{"decision":"approved","note":"Confirmed"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{decided}");
    let (status, _, _) = call(
        &state,
        "POST",
        &format!("/workspaces/{ws}/handoffs/{h1_id}/decision"),
        Some(&cookie),
        Some(r#"{"decision":"rejected"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "a decision is final");

    let (_, latest) = agent_send(&state, "GET", &format!("{agent}/handoffs/latest"), codex, None).await;
    assert_eq!(latest["approved"]["id"], h1_id.as_str());
    assert_eq!(latest["approved"]["stale"], false);
    assert_eq!(latest["approved"]["decisions"][0], "Audience: solo founders");

    let (status, v2) = agent_send(
        &state,
        "PUT",
        &format!("{agent}/files/{file_id}"),
        codex,
        Some(format!(r#"{{"request_id":"{}","base_version_id":"{v1}","content":"v2"}}"#, request_uuid(43))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, latest) = agent_send(&state, "GET", &format!("{agent}/handoffs/latest"), codex, None).await;
    assert_eq!(latest["approved"]["stale"], true, "changed evidence is flagged before the next delivery");
    assert_eq!(latest["approved"]["sources"][0]["current_version_id"], v2["version_id"]);

    let (_, h2) = agent_send(&state, "POST", &format!("{agent}/handoffs"), codex, handoff(44, "Old evidence", &v1)).await;
    let (status, body, _) = call(
        &state,
        "POST",
        &format!("/workspaces/{ws}/handoffs/{}/decision", h2["id"].as_str().unwrap()),
        Some(&cookie),
        Some(r#"{"decision":"approved"}"#),
    )
    .await;
    assert_eq!((status, body.as_str()), (StatusCode::CONFLICT, r#"{"error":"stale_sources"}"#));

    let (status, body) = agent_send(
        &state,
        "POST",
        &format!("{agent}/handoffs"),
        codex,
        Some(format!(
            r#"{{"request_id":"{}","summary":"s","next_step":"n","sources":[{{"file_id":"{file_id}","version_id":"00000000-0000-4000-8000-000000000000"}}]}}"#,
            request_uuid(45)
        )),
    )
    .await;
    assert_eq!((status, body["error"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("unknown_source")));
    println!("PASS handoffs: agent proposes with exact sources, read key cannot write, agents cannot decide, owner approval final, stale flag after source change, stale approval refused, unknown source rejected");
}
