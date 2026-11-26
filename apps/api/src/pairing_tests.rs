async fn plain_post(state: &AppState, uri: &str, body: String, bearer: Option<&str>) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let response = app(state.clone())
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
}

#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private PostgreSQL instance"]
async fn pairing_flow_against_live_database() {
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
    for email in ["pair-owner@live.test", "pair-other@live.test"] {
        let creds = format!(r#"{{"email":"{email}","password":"correct horse battery staple"}}"#);
        let (_, _, cookie) = call(&state, "POST", "/auth/register", None, Some(&creds)).await;
        cookies.push(cookie.unwrap());
    }
    let (_, body, _) = call(&state, "POST", "/workspaces", Some(&cookies[0]), Some(r#"{"name":"Paired"}"#)).await;
    let ws = json_field(&body, "id");

    // What the client does: generate a secret locally, send only its hash.
    let token = format!("aidrive_agent_{}", "c".repeat(64));
    let hash: String = crate::auth::token_hash(&token).iter().map(|b| format!("{b:02x}")).collect();
    let pairing = request_uuid(70);
    let start = format!(r#"{{"pairing_id":"{pairing}","client_name":"Codex desktop","token_sha256":"{hash}"}}"#);
    let (status, started) = plain_post(&state, "/agent/pairing", start.clone(), None).await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let code = started["user_code"].as_str().unwrap().to_owned();
    assert_eq!(started["verification_url"], format!("http://localhost:3000/pair?code={code}"));
    let (status, replay) = plain_post(&state, "/agent/pairing", start, None).await;
    assert_eq!((status, replay["user_code"].as_str()), (StatusCode::CREATED, Some(code.as_str())));

    let status_uri = format!("/agent/pairing/{pairing}/status");
    let (status, body) = plain_post(&state, &status_uri, String::new(), Some(&token)).await;
    assert_eq!((status, body["status"].as_str()), (StatusCode::OK, Some("pending")));
    let other = format!("aidrive_agent_{}", "d".repeat(64));
    let (status, _) = plain_post(&state, &status_uri, String::new(), Some(&other)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "status needs the client's own secret");

    let (status, body, _) = call(&state, "GET", &format!("/pairing/{code}"), Some(&cookies[0]), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("Codex desktop"));
    let decision = format!(r#"{{"decision":"approved","workspace_id":"{ws}","scope":"files:write"}}"#);
    let (status, _, _) = call(&state, "POST", &format!("/pairing/{code}/decision"), Some(&cookies[1]), Some(&decision)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "someone else cannot approve into a workspace they do not own");
    let (status, body, _) = call(&state, "POST", &format!("/pairing/{code}/decision"), Some(&cookies[0]), Some(&decision)).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = plain_post(&state, &status_uri, String::new(), Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "approved");
    assert_eq!(body["workspace_id"], ws.as_str());
    assert_eq!(body["workspace_name"], "Paired");
    assert_eq!(body["approver_email"], "pair-owner@live.test");
    let (status, _) = agent_send(&state, "GET", &format!("/agent/workspaces/{ws}/files"), &token, None).await;
    assert_eq!(status, StatusCode::OK, "the locally generated secret now works as an agent key");

    let (status, _, _) = call(&state, "POST", &format!("/pairing/{code}/decision"), Some(&cookies[0]), Some(&decision)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _, _) = call(&state, "GET", "/pairing/BBBBBBBB", Some(&cookies[0]), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    println!("PASS pairing: hash-only start, idempotent replay, secret-bound status, owner-only approval into own workspace, paired secret works as agent key, decided once");
}
