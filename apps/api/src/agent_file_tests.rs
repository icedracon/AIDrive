// Bearer-only requests, as an MCP client sends them: no cookie, no Fetch Metadata.
async fn agent_send(
    state: &AppState,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<String>,
) -> (StatusCode, serde_json::Value) {
    let response = app(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(body.map(Body::from).unwrap_or_else(Body::empty))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

fn request_uuid(n: u32) -> String {
    format!("20000000-0000-4000-8000-{n:012}")
}

#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private PostgreSQL instance"]
async fn agent_files_flow_against_live_database() {
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
    let creds = r#"{"email":"mcp-owner@live.test","password":"correct horse battery staple"}"#;
    let (status, body, cookie) = call(&state, "POST", "/auth/register", None, Some(creds)).await;
    assert_eq!(status, StatusCode::CREATED);
    let owner = json_field(&body, "id");
    let cookie = cookie.unwrap();
    let (_, body, _) = call(&state, "POST", "/workspaces", Some(&cookie), Some(r#"{"name":"MCP","review":"strict"}"#)).await;
    let ws = json_field(&body, "id");
    let (_, body, _) = call(&state, "POST", "/workspaces", Some(&cookie), Some(r#"{"name":"Other","review":"strict"}"#)).await;
    let other_ws = json_field(&body, "id");

    let mut tokens = Vec::new();
    for (n, scope) in [(1, "files:read"), (2, "files:write")] {
        let payload = format!(
            r#"{{"request_id":"{}","name":"agent {n}","scope":"{scope}"}}"#,
            request_uuid(n)
        );
        let (status, body, _) = call(
            &state,
            "POST",
            &format!("/workspaces/{ws}/agents"),
            Some(&cookie),
            Some(&payload),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["grant"]["scope"], scope);
        tokens.push(body["token"].as_str().unwrap().to_owned());
    }
    let (reader, writer) = (&tokens[0], &tokens[1]);
    let files = format!("/agent/workspaces/{ws}/files");

    let (status, created) = agent_send(
        &state,
        "POST",
        &files,
        writer,
        Some(format!(
            r#"{{"request_id":"{}","name":"handoff.md","content":"Decision: audience is solo founders"}}"#,
            request_uuid(10)
        )),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["agent_id"], request_uuid(2));
    assert_eq!(created["actor_id"], owner.as_str());
    let file = created["file_id"].as_str().unwrap().to_owned();
    let v1 = created["version_id"].as_str().unwrap().to_owned();

    let (status, listed) = agent_send(&state, "GET", &files, reader, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["files"].as_array().unwrap().len(), 1);
    let (status, read) = agent_send(&state, "GET", &format!("{files}/{file}"), reader, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["content"], "Decision: audience is solo founders");

    let edit = |n: u32, base: &str, text: &str| {
        Some(format!(
            r#"{{"request_id":"{}","base_version_id":"{base}","content":"{text}"}}"#,
            request_uuid(n)
        ))
    };
    let (status, body) = agent_send(&state, "PUT", &format!("{files}/{file}"), reader, edit(11, &v1, "x")).await;
    assert_eq!((status, body["error"].as_str()), (StatusCode::FORBIDDEN, Some("insufficient_scope")));
    let (status, v2) = agent_send(&state, "PUT", &format!("{files}/{file}"), writer, edit(12, &v1, "Next: interview five users")).await;
    assert_eq!(status, StatusCode::OK, "{v2}");
    assert_eq!(v2["revision"], 2);
    let (status, body) = agent_send(&state, "PUT", &format!("{files}/{file}"), writer, edit(13, &v1, "stale")).await;
    assert_eq!((status, body["error"].as_str()), (StatusCode::CONFLICT, Some("version_conflict")));

    let (status, _) = agent_send(&state, "GET", &format!("/agent/workspaces/{other_ws}/files"), writer, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "token is bound to its workspace");
    let (status, _, _) = call(&state, "GET", &files, Some(&cookie), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "agent routes ignore session cookies");

    let (status, history, _) = call(
        &state,
        "GET",
        &format!("/workspaces/{ws}/files/{file}/versions"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let history: serde_json::Value = serde_json::from_str(&history).unwrap();
    assert!(
        history["versions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["agent_id"] == request_uuid(2)),
        "owner sees which agent made each change: {history}"
    );

    let (status, _, _) = call(
        &state,
        "DELETE",
        &format!("/workspaces/{ws}/agents/{}", request_uuid(2)),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = agent_send(&state, "GET", &files, writer, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "revoked writer blocked on next request");

    let leaked: String = sqlx::query_scalar(
        "SELECT coalesce(current_setting('app.user_id', true), '') || coalesce(current_setting('app.agent_id', true), '')",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(leaked, "", "agent context leaked onto the pooled connection");
    println!("PASS agent files: bearer-only create/read/edit, scope levels, stale write, workspace binding, cookie separation, attribution, revocation, no context leak");
}
