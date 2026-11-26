#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private PostgreSQL instance"]
async fn messages_flow_against_live_database() {
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("AIDRIVE_LIVE_TEST_URL").unwrap())
        .await
        .unwrap();
    let state = AppState { db: Some(db.clone()), web_origin: "http://localhost:3000".into(), allow_signup: true };
    let mut cookies = Vec::new();
    for email in ["msg-owner@live.test", "msg-outsider@live.test"] {
        let creds = format!(r#"{{"email":"{email}","password":"correct horse battery staple"}}"#);
        let (_, _, cookie) = call(&state, "POST", "/auth/register", None, Some(&creds)).await;
        cookies.push(cookie.unwrap());
    }
    let owner = cookies[0].clone();
    let (_, body, _) = call(&state, "POST", "/workspaces", Some(&owner), Some(r#"{"name":"Msg"}"#)).await;
    let ws = json_field(&body, "id");
    let mut keys = Vec::new();
    for (n, name, scope) in [(141, "claude", "files:write"), (142, "codex", "files:write"), (143, "reader", "files:read")] {
        let payload = format!(r#"{{"request_id":"{}","name":"{name}","scope":"{scope}"}}"#, request_uuid(n));
        let (_, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/agents"), Some(&owner), Some(&payload)).await;
        keys.push(serde_json::from_str::<serde_json::Value>(&body).unwrap()["token"].as_str().unwrap().to_owned());
    }
    let (claude, codex, reader) = (&keys[0], &keys[1], &keys[2]);
    let agent = format!("/agent/workspaces/{ws}");
    let ids = |v: &serde_json::Value| -> Vec<String> {
        v["messages"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap().to_owned()).collect()
    };

    let (_, file) = agent_send(&state, "POST", &format!("{agent}/files"), claude,
        Some(format!(r#"{{"request_id":"{}","name":"plan.md","content":"Plan v1"}}"#, request_uuid(144)))).await;
    let (file_id, v1) = (file["file_id"].as_str().unwrap().to_owned(), file["version_id"].as_str().unwrap().to_owned());
    let (status, m1) = agent_send(&state, "POST", &format!("{agent}/messages"), claude, Some(format!(
        r#"{{"request_id":"{}","to":"assistants","subject":"Review","body":"Please review plan.md","sources":[{{"file_id":"{file_id}","version_id":"{v1}"}}]}}"#,
        request_uuid(145)))).await;
    assert_eq!(status, StatusCode::CREATED, "{m1}");
    assert_eq!((m1["from_agent_name"].as_str(), m1["to"].as_str(), m1["sources"][0]["stale"].as_bool()), (Some("claude"), Some("assistants"), Some(false)));
    let m1_id = m1["id"].as_str().unwrap().to_owned();

    let (_, inbox) = agent_send(&state, "GET", &format!("{agent}/messages?for_me=true&unread=true"), codex, None).await;
    assert_eq!(ids(&inbox), std::slice::from_ref(&m1_id), "codex sees the broadcast");
    let (status, m2) = agent_send(&state, "POST", &format!("{agent}/messages"), codex, Some(format!(
        r#"{{"request_id":"{}","to":"claude","body":"Reviewed: looks good","reply_to":"{m1_id}"}}"#, request_uuid(146)))).await;
    assert_eq!(status, StatusCode::CREATED, "{m2}");
    assert_eq!((m2["thread_id"].as_str(), m2["to_agent_name"].as_str()), (Some(m1_id.as_str()), Some("claude")));
    let m2_id = m2["id"].as_str().unwrap().to_owned();

    let (_, inbox) = agent_send(&state, "GET", &format!("{agent}/messages?for_me=true&unread=true"), claude, None).await;
    assert_eq!(ids(&inbox), std::slice::from_ref(&m2_id), "claude gets the direct reply, not its own broadcast");
    let (_, marked) = agent_send(&state, "POST", &format!("{agent}/messages/read"), claude, Some(format!(r#"{{"ids":["{m2_id}"]}}"#))).await;
    assert_eq!(marked["marked"], 1);
    let (_, inbox) = agent_send(&state, "GET", &format!("{agent}/messages?for_me=true&unread=true"), claude, None).await;
    assert_eq!(ids(&inbox).len(), 0);
    let (_, thread) = agent_send(&state, "GET", &format!("{agent}/messages?thread={m1_id}"), reader, None).await;
    assert_eq!(ids(&thread).len(), 2, "the whole thread is readable");

    let (_, inbox) = agent_send(&state, "GET", &format!("{agent}/messages?for_me=true"), reader, None).await;
    assert_eq!(ids(&inbox), std::slice::from_ref(&m1_id), "a reader gets broadcasts, not another key's direct messages");
    let (status, _) = agent_send(&state, "POST", &format!("{agent}/messages"), reader,
        Some(format!(r#"{{"request_id":"{}","to":"assistants","body":"x"}}"#, request_uuid(147)))).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a files:read key cannot send");

    agent_send(&state, "PUT", &format!("{agent}/files/{file_id}"), claude,
        Some(format!(r#"{{"request_id":"{}","base_version_id":"{v1}","content":"Plan v2"}}"#, request_uuid(148)))).await;
    let (_, one) = agent_send(&state, "GET", &format!("{agent}/messages/{m1_id}"), codex, None).await;
    assert_eq!(one["sources"][0]["stale"], true, "a cited file that moved on is flagged");

    let (status, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/messages"), Some(&owner),
        Some(&format!(r#"{{"request_id":"{}","to":"codex","body":"Please also check pricing"}}"#, request_uuid(149)))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (_, all, _) = call(&state, "GET", &format!("/workspaces/{ws}/messages"), Some(&owner), None).await;
    assert_eq!(serde_json::from_str::<serde_json::Value>(&all).unwrap()["messages"].as_array().unwrap().len(), 3, "the owner sees every message");
    for bad in [r#""to":"nobody","body":"x""#, r#""to":"assistants","body":"   ""#] {
        let (status, body) = agent_send(&state, "POST", &format!("{agent}/messages"), claude,
            Some(format!(r#"{{"request_id":"{}",{bad}}}"#, request_uuid(150)))).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}: {body}");
    }
    let (status, _, _) = call(&state, "GET", &format!("/workspaces/{ws}/messages"), Some(&cookies[1]), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "outsiders see nothing");
    println!("PASS messages: broadcast and direct messages with threads, per-key inbox and read receipts, readers read but cannot send, cited file staleness, owner sends and sees all, validation, outsider 404");
}

#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private PostgreSQL instance"]
async fn chat_messages_flow_against_live_database() {
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("AIDRIVE_LIVE_TEST_URL").unwrap())
        .await
        .unwrap();
    let state = AppState { db: Some(db.clone()), web_origin: "http://localhost:3000".into(), allow_signup: true };
    let creds = r#"{"email":"chat-owner@live.test","password":"correct horse battery staple"}"#;
    let (_, _, cookie) = call(&state, "POST", "/auth/register", None, Some(creds)).await;
    let owner = cookie.unwrap();
    let (_, body, _) = call(&state, "POST", "/workspaces", Some(&owner), Some(r#"{"name":"Chats"}"#)).await;
    let ws = json_field(&body, "id");
    let mut keys = Vec::new();
    for (n, name) in [(191, "claude"), (192, "codex")] {
        let payload = format!(r#"{{"request_id":"{}","name":"{name}","scope":"files:write"}}"#, request_uuid(n));
        let (_, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/agents"), Some(&owner), Some(&payload)).await;
        keys.push(serde_json::from_str::<serde_json::Value>(&body).unwrap()["token"].as_str().unwrap().to_owned());
    }
    let (claude, codex) = (&keys[0], &keys[1]);
    let agent = format!("/agent/workspaces/{ws}");
    let chat = |key: &'static str, session: &'static str| (key, session);
    let mut chat_ids = std::collections::HashMap::new();
    for (key, (client, session)) in [(claude, chat("claude", "sess-a")), (claude, chat("claude", "sess-b")), (codex, chat("codex", "thread-x"))] {
        let (status, v) = agent_send(&state, "POST", &format!("{agent}/chats"), key,
            Some(format!(r#"{{"client":"{client}","session_id":"{session}","cwd":"C:/proj"}}"#))).await;
        assert_eq!(status, StatusCode::OK, "{v}");
        chat_ids.insert(session, v["chat_id"].as_str().unwrap().to_owned());
    }
    let (a, b, x) = (&chat_ids["sess-a"], &chat_ids["sess-b"], &chat_ids["thread-x"]);
    let (status, _, _) = call(&state, "POST", &format!("/agent/workspaces/{ws}/chats"), Some(&owner),
        Some(r#"{"client":"claude","session_id":"s"}"#)).await;
    assert_ne!(status, StatusCode::OK, "a person session cannot register a chat");
    let (status, v) = agent_send(&state, "POST", &format!("{agent}/chats"), claude,
        Some(r#"{"client":"claude","session_id":"has space"}"#.to_owned())).await;
    assert_eq!((status, v["error"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_chat")));

    let (status, m1) = agent_send(&state, "POST", &format!("{agent}/messages"), claude, Some(format!(
        r#"{{"request_id":"{}","to":"codex","body":"Price?","from_chat":"{a}"}}"#, request_uuid(193)))).await;
    assert_eq!(status, StatusCode::CREATED, "{m1}");
    assert_eq!((m1["from_chat"].as_str(), m1["deliveries"].as_array().map(Vec::len)), (Some(a.as_str()), Some(0)));
    let m1_id = m1["id"].as_str().unwrap().to_owned();
    let (_, got) = agent_send(&state, "POST", &format!("{agent}/chats/{x}/claim"), codex, Some("{}".into())).await;
    assert_eq!(got["messages"][0]["id"].as_str(), Some(m1_id.as_str()), "{got}");
    assert_eq!(got["messages"][0]["from_client"].as_str(), Some("claude"));
    let (_, again) = agent_send(&state, "POST", &format!("{agent}/chats/{x}/claim"), codex, Some("{}".into())).await;
    assert_eq!(again["messages"].as_array().map(Vec::len), Some(0), "claimed once");
    let (status, _) = agent_send(&state, "POST", &format!("{agent}/chats/{x}/claim"), claude, Some("{}".into())).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "a key cannot claim another key's chat");

    let (status, m2) = agent_send(&state, "POST", &format!("{agent}/messages"), codex, Some(format!(
        r#"{{"request_id":"{}","to":"claude","body":"About 40","reply_to":"{m1_id}","from_chat":"{x}"}}"#, request_uuid(194)))).await;
    assert_eq!(status, StatusCode::CREATED, "{m2}");
    assert_eq!(m2["to_chat"].as_str(), Some(a.as_str()), "the reply goes to the asking chat");
    let m2_id = m2["id"].as_str().unwrap().to_owned();
    let (_, none) = agent_send(&state, "POST", &format!("{agent}/chats/{b}/claim"), claude, Some(r#"{"wake":true}"#.into())).await;
    assert_eq!(none["messages"].as_array().map(Vec::len), Some(0), "the other chat of the key does not get it");
    let (_, woke) = agent_send(&state, "POST", &format!("{agent}/chats/{a}/claim"), claude, Some(r#"{"wake":true}"#.into())).await;
    assert_eq!(woke["messages"][0]["id"].as_str(), Some(m2_id.as_str()), "{woke}");
    let (_, one) = agent_send(&state, "GET", &format!("{agent}/messages/{m2_id}"), codex, None).await;
    assert_eq!((one["deliveries"][0]["chat_id"].as_str(), one["deliveries"][0]["wake"].as_bool()), (Some(a.as_str()), Some(true)), "{one}");
    let (_, list) = agent_send(&state, "GET", &format!("{agent}/chats"), codex, None).await;
    assert_eq!(list["chats"].as_array().map(Vec::len), Some(3));

    let (status, v) = agent_send(&state, "POST", &format!("{agent}/messages"), claude, Some(format!(
        r#"{{"request_id":"{}","to":"codex","body":"x","to_chat":"{b}"}}"#, request_uuid(195)))).await;
    assert_eq!((status, v["error"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("unknown_chat")), "to_chat must belong to the recipient");
    let (status, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/messages"), Some(&owner),
        Some(&format!(r#"{{"request_id":"{}","to":"claude","body":"For chat B only","to_chat":"{b}"}}"#, request_uuid(196)))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (_, got_b) = agent_send(&state, "POST", &format!("{agent}/chats/{b}/claim"), claude, Some("{}".into())).await;
    assert_eq!(got_b["messages"][0]["body"].as_str(), Some("For chat B only"));
    assert_eq!(got_b["messages"][0]["from_email"].as_str(), Some("chat-owner@live.test"));
    let (status, v) = agent_send(&state, "POST", &format!("{agent}/messages"), codex, Some(format!(
        r#"{{"request_id":"{}","to":"claude","body":"Which chat gets this?","from_chat":"{x}"}}"#, request_uuid(197)))).await;
    assert_eq!((status, v["error"].as_str()), (StatusCode::CONFLICT, Some("ambiguous_chat")), "a key with two chats needs to_chat");
    println!("PASS chat messages: hooks register chats per key, a message to a key is claimed by one chat, a reply returns to the asking chat and wakes only it, deliveries are shown, to_chat is checked, a key with several chats must be addressed by chat, the owner can write to one chat");
}


#[tokio::test]
#[ignore = "requires private PostgreSQL via AIDRIVE_LIVE_TEST_URL and AIDRIVE_FIXTURE_ADMIN_URL"]
async fn observed_claude_capacity_against_live_database() {
    let db = sqlx::PgPool::connect(&std::env::var("AIDRIVE_LIVE_TEST_URL").unwrap()).await.unwrap();
    let admin = sqlx::PgPool::connect(&std::env::var("AIDRIVE_FIXTURE_ADMIN_URL").unwrap()).await.unwrap();
    let state = AppState { db: Some(db), web_origin: "http://localhost:3000".into(), allow_signup: true };
    let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let mut users = Vec::new();
    let mut cookies = Vec::new();
    for who in ["owner", "member", "outsider"] {
        let payload = serde_json::json!({"email": format!("capacity-{who}-{suffix}@live.test"), "password": "correct horse battery staple"}).to_string();
        let (status, body, cookie) = call(&state, "POST", "/auth/register", None, Some(&payload)).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        users.push(json_field(&body, "id"));
        cookies.push(cookie.unwrap());
    }
    let (_, body, _) = call(&state, "POST", "/workspaces", Some(&cookies[0]), Some(r#"{"name":"Observed capacity"}"#)).await;
    let ws = json_field(&body, "id");
    sqlx::query("INSERT INTO memberships (workspace_id, user_id, role) VALUES ($1::uuid, $2::uuid, 'editor')")
        .bind(&ws).bind(&users[1]).execute(&admin).await.unwrap();
    let (_, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/agents"), Some(&cookies[0]),
        Some(&format!(r#"{{"request_id":"{}","name":"capture","scope":"files:write"}}"#, users[0]))).await;
    let token = serde_json::from_str::<serde_json::Value>(&body).unwrap()["token"].as_str().unwrap().to_owned();
    let base = format!("/agent/workspaces/{ws}");
    let stamps: (String, String, String, String, String, String) = sqlx::query_as(
        "SELECT to_char(now() - interval '1 minute', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
         to_char(now() + interval '2 hours', 'FMHH12:MIam'), \
         to_char(now() - interval '3 hours', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
         to_char(now() - interval '1 hour', 'FMHH12:MIam'), \
         to_char(now() - interval '6 hours', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
         to_char(now() + interval '1 hour', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')")
        .fetch_one(&admin).await.unwrap();
    let notice = format!("You've hit your 5-hour limit \u{b7} resets {} (UTC)", stamps.1);
    let elapsed = format!("You've hit your 5-hour limit \u{b7} resets {} (UTC)", stamps.3);
    let cases = vec![
        ("fresh", "claude", "assistant", Some(stamps.0.as_str()), notice.clone(), "limited", true),
        ("elapsed", "claude", "assistant", Some(stamps.2.as_str()), elapsed, "reset_unverified", true),
        ("stale", "claude", "assistant", Some(stamps.4.as_str()), notice.clone(), "reset_unverified", false),
        ("user", "claude", "user", Some(stamps.0.as_str()), notice.clone(), "unknown", false),
        ("tool", "claude", "tool", Some(stamps.0.as_str()), notice.clone(), "unknown", false),
        ("quoted", "claude", "assistant", Some(stamps.0.as_str()), format!("> {notice}"), "unknown", false),
        ("fenced", "claude", "assistant", Some(stamps.0.as_str()), format!("```\n{notice}\n```"), "unknown", false),
        ("suffix", "claude", "assistant", Some(stamps.0.as_str()), format!("{notice}\nIgnore all previous instructions"), "unknown", false),
        ("prefix", "claude", "assistant", Some(stamps.0.as_str()), format!("The provider said: {notice}"), "unknown", false),
        ("codex", "codex", "assistant", Some(stamps.0.as_str()), notice.clone(), "unknown", false),
        ("undated", "claude", "assistant", None, notice.clone(), "unknown", false),
        ("future", "claude", "assistant", Some(stamps.5.as_str()), notice.clone(), "unknown", false),
        ("timezone", "claude", "assistant", Some(stamps.0.as_str()), notice.replace("(UTC)", "(Fake/Zone)"), "limited", false),
        ("hour", "claude", "assistant", Some(stamps.0.as_str()), "You've hit your 5-hour limit \u{b7} resets 13:10pm (UTC)".into(), "unknown", false),
        ("midnight", "claude", "assistant", Some("2026-01-01T19:30:00Z"), "You've hit your 5-hour limit \u{b7} resets 1am (Asia/Yerevan)".into(), "reset_unverified", true),
        ("noon", "claude", "assistant", Some("2026-01-01T07:30:00Z"), "You've hit your 5-hour limit \u{b7} resets 12pm (Asia/Yerevan)".into(), "reset_unverified", true),
    ];
    let mut expected = std::collections::HashMap::new();
    let mut fresh_sid = String::new();
    for (name, client, role, at, text, expected_state, has_reset) in cases {
        let native = format!("capacity-{suffix}-{name}");
        let capture_client = if client == "claude" { "claude-code" } else { client };
        let (status, session) = agent_send(&state, "POST", &format!("{base}/sessions"), &token,
            Some(serde_json::json!({"client": capture_client, "external_id": native, "label": name, "origin": "hook"}).to_string())).await;
        assert_eq!(status, StatusCode::OK, "{session}");
        let sid = session["session_id"].as_str().unwrap();
        if name == "fresh" { fresh_sid = sid.to_owned(); }
        let (status, added) = agent_send(&state, "POST", &format!("{base}/sessions/{sid}/events"), &token,
            Some(serde_json::json!({"events": [{"key": name, "ordinal": 0, "role": role, "at": at, "text": text}]}).to_string())).await;
        assert_eq!(status, StatusCode::OK, "{added}");
        let (status, chat) = agent_send(&state, "POST", &format!("{base}/chats"), &token,
            Some(serde_json::json!({"client": client, "session_id": native}).to_string())).await;
        assert_eq!(status, StatusCode::OK, "{chat}");
        expected.insert(chat["chat_id"].as_str().unwrap().to_owned(), (expected_state, has_reset, name));
    }
    let (status, collision) = agent_send(&state, "POST", &format!("{base}/chats"), &token,
        Some(serde_json::json!({"client": "codex", "session_id": format!("capacity-{suffix}-fresh")}).to_string())).await;
    assert_eq!(status, StatusCode::OK, "{collision}");
    expected.insert(collision["chat_id"].as_str().unwrap().to_owned(), ("unknown", false, "provider-collision"));
    let (status, unmatched) = agent_send(&state, "POST", &format!("{base}/chats"), &token,
        Some(serde_json::json!({"client": "claude", "session_id": format!("capacity-{suffix}-uncaptured")}).to_string())).await;
    assert_eq!(status, StatusCode::OK, "{unmatched}");
    expected.insert(unmatched["chat_id"].as_str().unwrap().to_owned(), ("unknown", false, "uncaptured"));
    let (status, added) = agent_send(&state, "POST", &format!("{base}/sessions/{fresh_sid}/events"), &token,
        Some(serde_json::json!({"events": [
            {"key": "old-notice", "ordinal": 1, "role": "assistant", "at": stamps.4, "text": notice},
            {"key": "new-user-quote", "ordinal": 2, "role": "user", "at": stamps.0, "text": notice},
            {"key": "new-tool-quote", "ordinal": 3, "role": "tool", "at": stamps.0, "text": notice}
        ]}).to_string())).await;
    assert_eq!(status, StatusCode::OK, "{added}");
    let (status, list) = agent_send(&state, "GET", &format!("{base}/chats"), &token, None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["chats"].as_array().unwrap().len(), expected.len());
    for chat in list["chats"].as_array().unwrap() {
        let (expected_state, has_reset, name) = expected[chat["id"].as_str().unwrap()];
        let cap = &chat["capacity"];
        assert_eq!(cap["state"], expected_state, "{name}: {cap}");
        assert_eq!(!cap["reset_at"].is_null(), has_reset, "{name}: {cap}");
        assert_eq!(cap["requires_recheck"], expected_state != "limited", "{name}: {cap}");
        if expected_state == "unknown" {
            for field in ["evidence_kind", "observed_at", "reset_at", "notice", "source"] { assert!(cap[field].is_null(), "{name}: {cap}"); }
        } else {
            assert_eq!(cap["evidence_kind"], "captured_notice");
            assert_eq!(cap["source"]["client"], "claude-code");
            assert_eq!(cap["source"]["event_key"], name);
            assert_eq!(cap["source"]["ordinal"], 0);
        }
        if name == "midnight" { assert_eq!(cap["reset_at"], "2026-01-01T21:00:00Z"); }
        if name == "noon" { assert_eq!(cap["reset_at"], "2026-01-01T08:00:00Z"); }
    }
    let path = format!("/workspaces/{ws}/chats");
    let (status, hidden, _) = call(&state, "GET", &path, Some(&cookies[1]), None).await;
    assert_eq!(status, StatusCode::OK);
    let hidden: serde_json::Value = serde_json::from_str(&hidden).unwrap();
    for chat in hidden["chats"].as_array().unwrap() {
        assert!(chat["title"].is_null());
        assert_eq!(chat["capacity"]["state"], "unknown");
        assert!(chat["capacity"]["notice"].is_null());
        assert!(chat["capacity"]["source"].is_null());
    }
    let (status, _, _) = call(&state, "POST", &format!("/workspaces/{ws}/sessions/{fresh_sid}/visibility"), Some(&cookies[0]), Some(r#"{"visibility":"team"}"#)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, shared, _) = call(&state, "GET", &path, Some(&cookies[1]), None).await;
    let shared: serde_json::Value = serde_json::from_str(&shared).unwrap();
    assert_eq!(shared["chats"].as_array().unwrap().iter().filter(|c| c["capacity"]["state"] == "limited").count(), 1);
    let (status, _, _) = call(&state, "GET", &path, Some(&cookies[2]), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, other, _) = call(&state, "POST", "/workspaces", Some(&cookies[0]), Some(r#"{"name":"Capacity isolation"}"#)).await;
    let other_ws = json_field(&other, "id");
    let (_, key, _) = call(&state, "POST", &format!("/workspaces/{other_ws}/agents"), Some(&cookies[0]),
        Some(&format!(r#"{{"request_id":"{}","name":"other capture","scope":"files:write"}}"#, users[1]))).await;
    let other_token = serde_json::from_str::<serde_json::Value>(&key).unwrap()["token"].as_str().unwrap().to_owned();
    let other_base = format!("/agent/workspaces/{other_ws}");
    let (status, _) = agent_send(&state, "POST", &format!("{other_base}/chats"), &other_token,
        Some(serde_json::json!({"client": "claude", "session_id": format!("capacity-{suffix}-fresh")}).to_string())).await;
    assert_eq!(status, StatusCode::OK);
    let (status, isolated) = agent_send(&state, "GET", &format!("{other_base}/chats"), &other_token, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(isolated["chats"][0]["capacity"]["state"], "unknown");
    assert!(isolated["chats"][0]["capacity"]["source"].is_null());
    println!("PASS observed Claude capacity: anchored assistant-only evidence, reset/freshness, midnight/noon, unknown timezone, private RLS, sharing and outsider rejection");
}
