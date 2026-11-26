#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private PostgreSQL instance"]
async fn session_memory_flow_against_live_database() {
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
    for email in ["sess-owner@live.test", "sess-outsider@live.test"] {
        let creds = format!(r#"{{"email":"{email}","password":"correct horse battery staple"}}"#);
        let (_, _, cookie) = call(&state, "POST", "/auth/register", None, Some(&creds)).await;
        cookies.push(cookie.unwrap());
    }
    let (_, body, _) = call(&state, "POST", "/workspaces", Some(&cookies[0]), Some(r#"{"name":"Memory"}"#)).await;
    let ws = json_field(&body, "id");
    let mut tokens = Vec::new();
    for (n, scope) in [(81, "files:write"), (82, "files:read")] {
        let payload = format!(r#"{{"request_id":"{}","name":"agent {n}","scope":"{scope}"}}"#, request_uuid(n));
        let (_, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/agents"), Some(&cookies[0]), Some(&payload)).await;
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        tokens.push(body["token"].as_str().unwrap().to_owned());
    }
    let (writer, reader) = (&tokens[0], &tokens[1]);
    let base = format!("/agent/workspaces/{ws}/sessions");

    let session = r#"{"client":"claude-code","external_id":"abc-123","label":"aidrive","origin":"import","started_at":"2026-11-19T10:00:00Z","ended_at":"2026-11-19T11:00:00Z"}"#;
    let (status, created) = agent_send(&state, "POST", &base, writer, Some(session.into())).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let sid = created["session_id"].as_str().unwrap().to_owned();
    let (_, again) = agent_send(&state, "POST", &base, writer, Some(session.into())).await;
    assert_eq!(again["session_id"], sid.as_str(), "re-capture reuses the session");

    let events = r#"{"complete":true,"events":[
      {"key":"u1","ordinal":0,"role":"user","at":"2026-11-19T10:00:00Z","text":"Which pricing model did we reject and why?"},
      {"key":"a1","ordinal":1,"role":"assistant","at":"2026-11-19T10:01:00Z","text":"We rejected per-seat pricing because solo founders are the pilot audience."},
      {"key":"t1","ordinal":2,"role":"tool","text":"tool: update_file pricing.md"}]}"#;
    let (status, added) = agent_send(&state, "POST", &format!("{base}/{sid}/events"), writer, Some(events.into())).await;
    assert_eq!((status, added["inserted"].as_i64()), (StatusCode::OK, Some(3)));
    let (_, added) = agent_send(&state, "POST", &format!("{base}/{sid}/events"), writer, Some(events.into())).await;
    assert_eq!(added["inserted"], 0, "replaying a batch inserts nothing");

    let (status, hits) = agent_send(&state, "GET", &format!("{base}/search?q=per-seat%20pricing"), reader, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hits[0]["ordinal"], 1);
    assert!(hits[0]["snippet"].as_str().unwrap().contains("«"), "{hits}");
    let (_, loose) = agent_send(&state, "GET", &format!("{base}/search?q=pricing%20nonexistentword"), reader, None).await;
    assert_eq!(loose[0]["ordinal"], 1, "any matching word is enough: {loose}");
    let (_, prefix) = agent_send(&state, "GET", &format!("{base}/search?q=pric"), reader, None).await;
    assert!(!prefix.as_array().unwrap().is_empty(), "terms match as prefixes: {prefix}");
    let (_, phrase) = agent_send(&state, "GET", &format!("{base}/search?q=%22seat%20founders%22"), reader, None).await;
    assert_eq!(phrase.as_array().unwrap().len(), 0, "quoted phrases still require the exact phrase");
    let (status, excerpt) = agent_send(&state, "GET", &format!("{base}/{sid}/events?around=1&radius=1"), reader, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(excerpt.as_array().unwrap().len(), 3);
    let (status, list) = agent_send(&state, "GET", &base, reader, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!((list[0]["coverage"].as_str(), list[0]["event_count"].as_i64()), (Some("complete"), Some(3)));

    let chat_body = r#"{"client":"claude","session_id":"abc-123","cwd":"C:/synthetic"}"#;
    let (status, chat) = agent_send(&state, "POST", &format!("/agent/workspaces/{ws}/chats"), writer, Some(chat_body.into())).await;
    assert_eq!(status, StatusCode::OK, "{chat}");
    let chat_id = chat["chat_id"].as_str().unwrap();
    let (_, registry) = agent_send(&state, "GET", &format!("/agent/workspaces/{ws}/chats"), reader, None).await;
    assert_eq!(registry["chats"][0]["title"], "aidrive");
    assert_eq!(registry["chats"][0]["session_id"], "abc-123");
    let transcript = format!("/agent/workspaces/{ws}/chats/{chat_id}/transcript");
    let (status, page) = agent_send(&state, "GET", &format!("{transcript}?limit=2"), reader, None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["session"]["id"], sid);
    assert_eq!(page["events"][0]["role"], "user");
    assert_eq!(page["events"][1]["role"], "assistant");
    assert_eq!(page["next_cursor"]["ordinal"], 1);
    let (status, last) = agent_send(&state, "GET", &format!("{transcript}?limit=2&after_ordinal=1&after_key=a1"), reader, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(last["events"].as_array().unwrap().len(), 1);
    assert_eq!(last["events"][0]["key"], "t1");
    assert!(last["next_cursor"].is_null());
    let (status, _, _) = call(&state, "GET", &format!("/workspaces/{ws}/chats/{chat_id}/transcript"), Some(&cookies[0]), None).await;
    assert_eq!(status, StatusCode::OK, "human web route reads the exact capture");
    let (status, _, _) = call(&state, "GET", &format!("/workspaces/{ws}/chats/{chat_id}/transcript"), Some(&cookies[1]), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "outsiders cannot read chat transcripts");
    let (status, _) = agent_send(&state, "GET", &format!("{transcript}?after_ordinal=0"), reader, None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let other_chat_body = r#"{"client":"codex","session_id":"abc-123","cwd":"C:/synthetic"}"#;
    let (_, other_chat) = agent_send(&state, "POST", &format!("/agent/workspaces/{ws}/chats"), writer, Some(other_chat_body.into())).await;
    let (status, _) = agent_send(&state, "GET", &format!("/agent/workspaces/{ws}/chats/{}/transcript", other_chat["chat_id"].as_str().unwrap()), reader, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "same ID on another provider must not fall back to Claude");
    let tied = r#"{"events":[{"key":"t2","ordinal":2,"role":"tool","text":"second event at same ordinal"}]}"#;
    agent_send(&state, "POST", &format!("{base}/{sid}/events"), writer, Some(tied.into())).await;
    let (status, tied_page) = agent_send(&state, "GET", &format!("{transcript}?limit=1&after_ordinal=2&after_key=t1"), reader, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tied_page["events"][0]["key"], "t2", "cursor must not drop equal ordinals");
    println!("PASS transcripts: exact native chat/provider, user+assistant events, stable cursor ties, human access and outsider rejection");

    let (status, body) = agent_send(&state, "POST", &base, reader, Some(session.into())).await;
    assert_eq!((status, body["error"].as_str()), (StatusCode::FORBIDDEN, Some("insufficient_scope")));
    let bad = r#"{"events":[{"key":"x","ordinal":0,"role":"system","text":"hidden instructions"}]}"#;
    let (status, _) = agent_send(&state, "POST", &format!("{base}/{sid}/events"), writer, Some(bad.into())).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "only user/assistant/tool roles are stored");
    let (status, _) = agent_send(&state, "DELETE", &format!("{base}/{sid}"), writer, None).await;
    assert!(matches!(status, StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED), "agents have no delete route");

    let (status, _, _) = call(&state, "GET", &format!("/workspaces/{ws}/sessions"), Some(&cookies[1]), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "outsiders cannot list sessions");
    let (status, _, _) = call(&state, "DELETE", &format!("/workspaces/{ws}/sessions/{sid}"), Some(&cookies[0]), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, hits) = agent_send(&state, "GET", &format!("{base}/search?q=pricing"), reader, None).await;
    assert_eq!(hits.as_array().unwrap().len(), 0, "deleted sessions are no longer searchable");
    println!("PASS sessions: capture idempotent, Armenian/English search with snippets, excerpt, coverage, scope and role checks, owner-only delete removes search hits");
}

#[tokio::test]
#[ignore = "scripts/with-test-postgres.sh supplies a private PostgreSQL instance"]
async fn session_transcript_segments_against_live_database() {
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("AIDRIVE_LIVE_TEST_URL").unwrap())
        .await
        .unwrap();
    let state = AppState {
        db: Some(db),
        web_origin: "http://localhost:3000".into(),
        allow_signup: true,
    };
    let (status, _, cookie) = call(
        &state,
        "POST",
        "/auth/register",
        None,
        Some(r#"{"email":"transcript-segments@live.test","password":"correct horse battery staple"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let cookie = cookie.unwrap();
    let (status, body, _) = call(
        &state,
        "POST",
        "/workspaces",
        Some(&cookie),
        Some(r#"{"name":"Transcript segments"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let ws = json_field(&body, "id");
    let grant = format!(
        r#"{{"request_id":"{}","name":"segment capture","scope":"files:write"}}"#,
        request_uuid(86)
    );
    let (status, body, _) = call(
        &state,
        "POST",
        &format!("/workspaces/{ws}/agents"),
        Some(&cookie),
        Some(&grant),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let grant: serde_json::Value = serde_json::from_str(&body).unwrap();
    let token = grant["token"].as_str().unwrap();
    let base = format!("/agent/workspaces/{ws}/sessions");
    let mut sid = None;
    for (day, prefix) in [(24, "z"), (25, "m"), (26, "a")] {
        let capture = serde_json::json!({
            "client": "codex", "external_id": "three-segment-session", "label": "Segmented Codex",
            "origin": "import", "started_at": format!("2026-11-{day}T10:00:00Z"),
            "ended_at": format!("2026-11-{day}T10:01:00Z"),
        });
        let (status, session) = agent_send(&state, "POST", &base, token, Some(capture.to_string())).await;
        assert_eq!(status, StatusCode::OK, "{session}");
        let session_id = session["session_id"].as_str().unwrap().to_owned();
        if let Some(previous) = &sid {
            assert_eq!(&session_id, previous, "all segments must retain the same session");
        }
        sid = Some(session_id.clone());
        let batch = serde_json::json!({ "events": [
            { "key": format!("{prefix}0"), "ordinal": 0, "role": "user", "at": format!("2026-11-{day}T10:00:00Z"), "text": format!("Day {day} question") },
            { "key": format!("{prefix}1"), "ordinal": 1, "role": "assistant", "at": format!("2026-11-{day}T10:01:00Z"), "text": format!("Day {day} answer") },
        ] });
        let (status, added) = agent_send(&state, "POST", &format!("{base}/{session_id}/events"), token, Some(batch.to_string())).await;
        assert_eq!(status, StatusCode::OK, "{added}");
        assert_eq!(added["inserted"], 2);
    }
    let sid = sid.unwrap();
    let extra = serde_json::json!({ "events": [
        { "key": "a2", "ordinal": 1, "role": "tool", "at": "2026-11-26T10:01:00Z", "text": "Same timestamp and stored ordinal" },
        { "key": "untimed", "ordinal": 0, "role": "tool", "text": "No timestamp" },
    ] });
    let (status, added) = agent_send(&state, "POST", &format!("{base}/{sid}/events"), token, Some(extra.to_string())).await;
    assert_eq!(status, StatusCode::OK, "{added}");
    assert_eq!(added["inserted"], 2);
    let (status, chat) = agent_send(&state, "POST", &format!("/agent/workspaces/{ws}/chats"), token, Some(
        r#"{"client":"codex","session_id":"three-segment-session","cwd":"C:/synthetic-segments"}"#.into(),
    )).await;
    assert_eq!(status, StatusCode::OK, "{chat}");
    let chat_id = chat["chat_id"].as_str().unwrap();
    let transcript = format!("/workspaces/{ws}/chats/{chat_id}/transcript");
    let expected = ["z0", "z1", "m0", "m1", "a0", "a1", "a2", "untimed"];
    let mut query = "limit=2".to_owned();
    let mut received = Vec::new();
    for offset in (0..expected.len()).step_by(2) {
        let (status, body, _) = call(&state, "GET", &format!("{transcript}?{query}"), Some(&cookie), None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let page: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(page["session"]["id"], sid);
        assert_eq!(page["session"]["event_count"], expected.len());
        let events = page["events"].as_array().unwrap();
        assert_eq!(events.len(), 2);
        for (index, event) in events.iter().enumerate() {
            assert_eq!(event["key"], expected[offset + index], "chronology must win over restarted ordinals and key order");
            assert_eq!(event["ordinal"], offset + index, "cursor ordinals must follow display order");
            received.push(event["key"].as_str().unwrap().to_owned());
        }
        if offset + 2 < expected.len() {
            let cursor = &page["next_cursor"];
            assert_eq!(cursor["ordinal"], offset + 1);
            assert_eq!(cursor["key"], expected[offset + 1]);
            query = format!("limit=2&after_ordinal={}&after_key={}", cursor["ordinal"], cursor["key"].as_str().unwrap());
        } else {
            assert!(page["next_cursor"].is_null());
            assert!(events[1]["at"].is_null(), "untimed events must sort last");
        }
    }
    assert_eq!(received, expected);
    for (ordinal, expected_count) in [(0, 4), (1, 4)] {
        let (status, excerpt) = agent_send(&state, "GET", &format!("{base}/{sid}/events?around={ordinal}&radius=0"), token, None).await;
        assert_eq!(status, StatusCode::OK, "{excerpt}");
        let events = excerpt.as_array().unwrap();
        assert_eq!(events.len(), expected_count, "transcript reads must not rewrite stored ordinals");
        assert!(events.iter().all(|event| event["ordinal"] == ordinal));
    }
    println!("PASS transcript segments: chronological HTTP pagination across restarted ordinals, stable keys, timestamp ties, NULLS LAST, stored ordinals unchanged");
}
