#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private PostgreSQL instance"]
async fn workspace_summary_flow_against_live_database() {
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("AIDRIVE_LIVE_TEST_URL").unwrap())
        .await
        .unwrap();
    let admin = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("AIDRIVE_FIXTURE_ADMIN_URL").unwrap())
        .await
        .unwrap();
    let state = AppState { db: Some(db.clone()), web_origin: "http://localhost:3000".into(), allow_signup: true };
    let (mut users, mut cookies) = (Vec::new(), Vec::new());
    for email in ["summary-owner@live.test", "summary-editor@live.test", "summary-viewer@live.test", "summary-outsider@live.test"] {
        let creds = format!(r#"{{"email":"{email}","password":"correct horse battery staple"}}"#);
        let (status, body, cookie) = call(&state, "POST", "/auth/register", None, Some(&creds)).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        users.push(json_field(&body, "id"));
        cookies.push(cookie.unwrap());
    }
    let (owner, editor, viewer, outsider) = (&cookies[0], &cookies[1], &cookies[2], &cookies[3]);
    let json = |body: &str| serde_json::from_str::<serde_json::Value>(body).unwrap();
    let create = |cookie: &str, name: &str| {
        let state = state.clone();
        let (cookie, payload) = (cookie.to_owned(), format!(r#"{{"name":"{name}","review":"strict"}}"#));
        async move { json_field(&call(&state, "POST", "/workspaces", Some(&cookie), Some(&payload)).await.1, "id") }
    };
    let one = |cookie: &str, ws: &str| {
        let state = state.clone();
        let (cookie, uri) = (cookie.to_owned(), format!("/workspaces/{ws}/summary"));
        async move {
            let (status, body, _) = call(&state, "GET", &uri, Some(&cookie), None).await;
            (status, if status == StatusCode::OK { json(&body)["summary"].clone() } else { serde_json::Value::Null })
        }
    };
    let all = |cookie: &str| {
        let state = state.clone();
        let cookie = cookie.to_owned();
        async move {
            let (status, body, _) = call(&state, "GET", "/workspaces/summaries", Some(&cookie), None).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            json(&body)["summaries"].as_array().unwrap().clone()
        }
    };

    let ws = create(owner, "Summary launch").await;
    let empty_ws = create(owner, "Summary empty").await;
    let foreign = create(outsider, "Summary foreign").await;

    let (status, s) = one(owner, &ws).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(s, serde_json::json!({"id": ws, "name": "Summary launch", "role": "owner", "file_count": 0,
        "registered_chat_count": 0, "last_visible_activity_at": null}), "a new space has zero counts and no invented activity");

    for (user, role) in [(&users[1], "editor"), (&users[2], "viewer")] {
        sqlx::query("INSERT INTO memberships (workspace_id, user_id, role) VALUES ($1::uuid, $2::uuid, $3)")
            .bind(&ws).bind(user).bind(role).execute(&admin).await.unwrap();
    }
    let mut tokens = Vec::new();
    for (n, w, cookie) in [(211, &ws, owner), (212, &foreign, outsider)] {
        let payload = format!(r#"{{"request_id":"{}","name":"Codex","scope":"files:write"}}"#, request_uuid(n));
        let (status, body, _) = call(&state, "POST", &format!("/workspaces/{w}/agents"), Some(cookie), Some(&payload)).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        tokens.push(json(&body)["token"].as_str().unwrap().to_owned());
    }
    let (status, after_key) = one(owner, &ws).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!((after_key["file_count"].as_i64(), after_key["registered_chat_count"].as_i64()), (Some(0), Some(0)));
    let key_at = after_key["last_visible_activity_at"].as_str().expect("issuing a key is visible activity").to_owned();

    for (n, w, token, name) in [(213, &ws, &tokens[0], "plan.md"), (214, &ws, &tokens[0], "notes.md"), (215, &foreign, &tokens[1], "secret.md")] {
        let (status, body) = agent_send(&state, "POST", &format!("/agent/workspaces/{w}/files"), token,
            Some(format!(r#"{{"request_id":"{}","name":"{name}","content":"hello"}}"#, request_uuid(n)))).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    for (w, token, session) in [(&ws, &tokens[0], "summary-thread-1"), (&foreign, &tokens[1], "summary-foreign-1"), (&foreign, &tokens[1], "summary-foreign-2")] {
        let (status, body) = agent_send(&state, "POST", &format!("/agent/workspaces/{w}/chats"), token,
            Some(format!(r#"{{"client":"codex","session_id":"{session}"}}"#))).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    let mut team_at = String::new();
    for (cookie, role) in [(owner, "owner"), (editor, "editor"), (viewer, "viewer")] {
        let (status, s) = one(cookie, &ws).await;
        assert_eq!(status, StatusCode::OK, "{role}");
        assert_eq!(s["role"], role);
        assert_eq!((s["file_count"].as_i64(), s["registered_chat_count"].as_i64()), (Some(2), Some(1)),
            "{role} sees the real counts of this space only: {s}");
        let at = s["last_visible_activity_at"].as_str().unwrap().to_owned();
        assert!(at > key_at, "a synthetic file create moves the activity time: {at} vs {key_at}");
        if team_at.is_empty() { team_at = at } else { assert_eq!(at, team_at, "{role} sees the same team activity") }
        assert!(s.get("token").is_none() && !s.to_string().contains(&tokens[0]), "no credentials in the summary");
    }

    let (status, body) = agent_send(&state, "POST", &format!("/agent/workspaces/{ws}/knowledge"), &tokens[0],
        Some(format!(r#"{{"request_id":"{}","kind":"fact","topic":"pricing","statement":"Owner-only draft","visibility":"private"}}"#, request_uuid(216)))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (_, owner_view) = one(owner, &ws).await;
    let (_, viewer_view) = one(viewer, &ws).await;
    assert!(owner_view["last_visible_activity_at"].as_str().unwrap() > team_at.as_str(), "the author sees their private proposal as activity");
    assert_eq!(viewer_view["last_visible_activity_at"].as_str(), Some(team_at.as_str()), "others' private knowledge never moves the time");

    let (status, _) = one(outsider, &ws).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "outsiders get the same 404 as unknown spaces");
    let (status, _) = one(owner, &foreign).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "membership in one space gives nothing in another");
    let (status, _) = one(owner, "00000000-0000-4000-8000-0000000000ff").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = one(owner, "not-a-uuid").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = call(&state, "GET", "/workspaces/summaries", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let batch = all(owner).await;
    let ids: Vec<&str> = batch.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec![ws.as_str(), empty_ws.as_str()], "only the caller's spaces, in list order");
    assert_eq!(batch[0], owner_view, "the batched entry equals the single summary");
    assert_eq!((batch[1]["file_count"].as_i64(), batch[1]["registered_chat_count"].as_i64(), batch[1]["last_visible_activity_at"].is_null()),
        (Some(0), Some(0), true), "scope switches per space: the empty space does not inherit counts");
    let viewer_batch = all(viewer).await;
    assert_eq!(viewer_batch.len(), 1);
    assert_eq!(viewer_batch[0], viewer_view);
    let outsider_batch = all(outsider).await;
    assert_eq!(outsider_batch.len(), 1);
    assert_eq!((outsider_batch[0]["id"].as_str(), outsider_batch[0]["file_count"].as_i64(), outsider_batch[0]["registered_chat_count"].as_i64()),
        (Some(foreign.as_str()), Some(1), Some(2)), "foreign space counts stay in the foreign space");
    println!("PASS workspace summary: owner/editor/viewer see real file and registered-chat counts and visible activity; empty spaces report zero and null; a file create moves the time; private knowledge moves it only for its author; outsiders and foreign spaces 404; the batched list is scoped per space and equals the single view");
}
