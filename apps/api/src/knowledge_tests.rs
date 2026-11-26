#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private PostgreSQL instance"]
async fn knowledge_flow_against_live_database() {
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
    for email in ["know-owner@live.test", "know-outsider@live.test"] {
        let creds = format!(r#"{{"email":"{email}","password":"correct horse battery staple"}}"#);
        let (_, _, cookie) = call(&state, "POST", "/auth/register", None, Some(&creds)).await;
        cookies.push(cookie.unwrap());
    }
    let (_, body, _) = call(&state, "POST", "/workspaces", Some(&cookies[0]), Some(r#"{"name":"Know","review":"strict"}"#)).await;
    let ws = json_field(&body, "id");
    let payload = format!(r#"{{"request_id":"{}","name":"claude","scope":"files:write"}}"#, request_uuid(91));
    let (_, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/agents"), Some(&cookies[0]), Some(&payload)).await;
    let token = serde_json::from_str::<serde_json::Value>(&body).unwrap()["token"].as_str().unwrap().to_owned();
    let agent = format!("/agent/workspaces/{ws}");

    let (_, file) = agent_send(&state, "POST", &format!("{agent}/files"), &token,
        Some(format!(r#"{{"request_id":"{}","name":"pricing.md","content":"Flat price"}}"#, request_uuid(92)))).await;
    let (file_id, v1) = (file["file_id"].as_str().unwrap().to_owned(), file["version_id"].as_str().unwrap().to_owned());
    let (_, session) = agent_send(&state, "POST", &format!("{agent}/sessions"), &token,
        Some(r#"{"client":"codex","external_id":"k1","label":"p","origin":"import"}"#.into())).await;
    let sid = session["session_id"].as_str().unwrap().to_owned();
    agent_send(&state, "POST", &format!("{agent}/sessions/{sid}/events"), &token,
        Some(r#"{"complete":true,"events":[{"key":"e0","ordinal":0,"role":"user","text":"No per-seat pricing"}]}"#.into())).await;

    let proposal = format!(
        r#"{{"request_id":"{}","kind":"decision","topic":"pricing","statement":"Use a flat price","evidence":[{{"file_id":"{file_id}","version_id":"{v1}","quote":"Flat price"}},{{"session_id":"{sid}","ordinal":0}}]}}"#,
        request_uuid(93)
    );
    let (status, item) = agent_send(&state, "POST", &format!("{agent}/knowledge"), &token, Some(proposal)).await;
    assert_eq!(status, StatusCode::CREATED, "{item}");
    assert_eq!(item["status"], "proposed");
    assert_eq!(item["evidence"].as_array().unwrap().len(), 2);
    assert_eq!(item["evidence"][1]["event_key"], "e0", "ordinal resolved to the stable event key");
    let item_id = item["id"].as_str().unwrap().to_owned();

    let (_, current) = agent_send(&state, "GET", &format!("{agent}/knowledge"), &token, None).await;
    assert_eq!(current["items"].as_array().unwrap().len(), 0, "proposals are not current knowledge");
    let (status, _) = agent_send(&state, "POST", &format!("{agent}/knowledge/{item_id}/decision"), &token, Some(r#"{"decision":"approved"}"#.into())).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a low (files:write) key cannot decide");

    let (status, inbox, _) = call(&state, "GET", &format!("/workspaces/{ws}/knowledge?status=proposed"), Some(&cookies[0]), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(inbox.contains(&item_id));
    let (status, approved, _) = call(&state, "POST", &format!("/workspaces/{ws}/knowledge/{item_id}/decision"), Some(&cookies[0]),
        Some(r#"{"decision":"approved","statement":"Use one flat monthly price"}"#)).await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    let approved: serde_json::Value = serde_json::from_str(&approved).unwrap();
    assert_eq!((approved["status"].as_str(), approved["statement"].as_str()), (Some("approved"), Some("Use one flat monthly price")));
    assert_ne!(approved["id"], item_id.as_str(), "an edit is a new revision");

    let (_, current) = agent_send(&state, "GET", &format!("{agent}/knowledge?q=monthly"), &token, None).await;
    assert_eq!(current["items"][0]["statement"], "Use one flat monthly price");
    assert_eq!(current["items"][0]["stale"], false);
    let (_, short) = agent_send(&state, "GET", &format!("{agent}/knowledge?q=we%20monthly"), &token, None).await;
    assert_eq!(short["items"][0]["statement"], "Use one flat monthly price", "a short word before a term must not break the query");
    let (_, excluded) = agent_send(&state, "GET", &format!("{agent}/knowledge?q=monthly%20-flat"), &token, None).await;
    assert_eq!(excluded["items"].as_array().map(Vec::len), Some(0), "an excluded term stays excluded");
    agent_send(&state, "PUT", &format!("{agent}/files/{file_id}"), &token,
        Some(format!(r#"{{"request_id":"{}","base_version_id":"{v1}","content":"Flat price, annual discount"}}"#, request_uuid(94)))).await;
    let (_, current) = agent_send(&state, "GET", &format!("{agent}/knowledge"), &token, None).await;
    assert_eq!(current["items"][0]["stale"], true, "approved knowledge is flagged once its file evidence changes");
    let (_, history) = agent_send(&state, "GET", &format!("{agent}/knowledge?status=all"), &token, None).await;
    assert!(history["items"].as_array().unwrap().iter().any(|i| i["status"] == "superseded"), "original wording kept as history");

    let (status, _) = agent_send(&state, "POST", &format!("{agent}/knowledge"), &token,
        Some(format!(r#"{{"request_id":"{}","kind":"attempt","statement":"Tried per-seat"}}"#, request_uuid(95)))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "attempts need an outcome");
    let (status, _) = agent_send(&state, "POST", &format!("{agent}/knowledge"), &token,
        Some(format!(r#"{{"request_id":"{}","kind":"fact","statement":"C:\\AIDrive\bin"}}"#, request_uuid(97)))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "control characters (here a stray backspace) are rejected");
    let (status, body) = agent_send(&state, "POST", &format!("{agent}/knowledge"), &token,
        Some(format!(r#"{{"request_id":"{}","kind":"fact","statement":"x","evidence":[{{"session_id":"{sid}","ordinal":9}}]}}"#, request_uuid(96)))).await;
    assert_eq!((status, body["error"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("unknown_evidence")));
    let (status, _, _) = call(&state, "GET", &format!("/workspaces/{ws}/knowledge"), Some(&cookies[1]), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "outsiders cannot read knowledge");

    for n in 0..5u32 {
        let (status, _) = agent_send(&state, "POST", &format!("{agent}/knowledge"), &token,
            Some(format!(r#"{{"request_id":"{}","kind":"fact","topic":"page {n}","statement":"Paged fact {n}"}}"#, request_uuid(401 + n)))).await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let mut seen = Vec::new();
    for offset in [0, 2, 4] {
        let (status, page, _) = call(&state, "GET", &format!("/workspaces/{ws}/knowledge?status=proposed&limit=2&offset={offset}"), Some(&cookies[0]), None).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let page: serde_json::Value = serde_json::from_str(&page).unwrap();
        assert_eq!(page["total"], 5, "total counts every pending proposal, not just this page");
        seen.extend(page["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_owned()));
    }
    let unique: std::collections::HashSet<_> = seen.iter().collect();
    assert_eq!((seen.len(), unique.len()), (5, 5), "pages neither repeat nor skip an item");
    let (_, past_end, _) = call(&state, "GET", &format!("/workspaces/{ws}/knowledge?status=proposed&limit=2&offset=10"), Some(&cookies[0]), None).await;
    let past_end: serde_json::Value = serde_json::from_str(&past_end).unwrap();
    assert_eq!((past_end["items"].as_array().map(Vec::len), past_end["total"].as_i64()), (Some(0), Some(5)));
    for bad in ["offset=-1", "offset=100001", "offset=2&q=paged"] {
        let (status, _, _) = call(&state, "GET", &format!("/workspaces/{ws}/knowledge?status=proposed&{bad}"), Some(&cookies[0]), None).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    }
    let (_, searched) = agent_send(&state, "GET", &format!("{agent}/knowledge?status=proposed&q=paged"), &token, None).await;
    assert!(searched.get("total").is_none(), "search results report no total");
    println!("PASS knowledge: agent proposes with file and session evidence, proposals not current, low keys cannot decide, owner edit-approve as new revision, search, stale after file change, history kept, validation, paging with total");
}

#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private PostgreSQL instance"]
async fn knowledge_validity_flow_against_live_database() {
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("AIDRIVE_LIVE_TEST_URL").unwrap())
        .await
        .unwrap();
    let state = AppState { db: Some(db.clone()), web_origin: "http://localhost:3000".into(), allow_signup: true };
    let creds = r#"{"email":"valid-owner@live.test","password":"correct horse battery staple"}"#;
    let (_, _, cookie) = call(&state, "POST", "/auth/register", None, Some(creds)).await;
    let cookie = cookie.unwrap();
    let (_, body, _) = call(&state, "POST", "/workspaces", Some(&cookie), Some(r#"{"name":"Validity","review":"strict"}"#)).await;
    let ws = json_field(&body, "id");
    let payload = format!(r#"{{"request_id":"{}","name":"codex","scope":"files:write"}}"#, request_uuid(101));
    let (_, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/agents"), Some(&cookie), Some(&payload)).await;
    let token = serde_json::from_str::<serde_json::Value>(&body).unwrap()["token"].as_str().unwrap().to_owned();
    let (agent, owner) = (format!("/agent/workspaces/{ws}/knowledge"), format!("/workspaces/{ws}/knowledge"));
    let decide = |item: &str, body: &str| {
        let (path, body, cookie, state) = (format!("{owner}/{item}/decision"), body.to_owned(), cookie.clone(), state.clone());
        async move {
            let (status, text, _) = call(&state, "POST", &path, Some(&cookie), Some(&body)).await;
            (status, serde_json::from_str::<serde_json::Value>(&text).unwrap_or_default())
        }
    };

    let (_, d1) = agent_send(&state, "POST", &agent, &token,
        Some(format!(r#"{{"request_id":"{}","kind":"decision","topic":"pricing","statement":"Flat price"}}"#, request_uuid(102)))).await;
    let d1 = d1["id"].as_str().unwrap().to_owned();
    let (status, approved) = decide(&d1, r#"{"decision":"approved"}"#).await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    let d1_from = approved["valid_from"].as_str().unwrap().to_owned();
    assert!(approved["invalid_at"].is_null());

    let (_, p1) = agent_send(&state, "POST", &agent, &token,
        Some(format!(r#"{{"request_id":"{}","kind":"decision","topic":"Pricing","statement":"Per-seat price"}}"#, request_uuid(103)))).await;
    let p1 = p1["id"].as_str().unwrap().to_owned();
    assert_eq!(p1.len(), 36);
    let (_, current) = agent_send(&state, "GET", &agent, &token, None).await;
    assert_eq!(current["items"][0]["disputed_by"], serde_json::json!([p1]), "assistants see the challenged decision as disputed");
    let (_, inbox, _) = call(&state, "GET", &format!("{owner}?status=proposed"), Some(&cookie), None).await;
    let inbox: serde_json::Value = serde_json::from_str(&inbox).unwrap();
    assert_eq!(inbox["items"][0]["conflicts"], serde_json::json!([d1]), "the owner sees what approval would contradict");

    let (status, body) = decide(&p1, r#"{"decision":"approved"}"#).await;
    assert_eq!((status, body["error"].as_str()), (StatusCode::CONFLICT, Some("conflict_unresolved")));
    let (status, _) = decide(&p1, r#"{"decision":"approved","resolution":"merge"}"#).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, body) = decide(&p1, r#"{"decision":"approved","resolution":"replace"}"#).await;
    assert_eq!((status, body["status"].as_str()), (StatusCode::OK, Some("approved")), "{body}");

    let (_, current) = agent_send(&state, "GET", &agent, &token, None).await;
    let ids: Vec<&str> = current["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [p1.as_str()], "current recall shows only the replacement");
    assert_eq!(current["items"][0]["disputed_by"], serde_json::json!([]));
    let (_, history) = agent_send(&state, "GET", &format!("{agent}?status=all"), &token, None).await;
    let old = history["items"].as_array().unwrap().iter().find(|i| i["id"] == d1.as_str()).unwrap();
    assert_eq!((old["status"].as_str(), old["superseded_by"].as_str()), (Some("superseded"), Some(p1.as_str())));
    assert!(old["invalid_at"].as_str().unwrap() > d1_from.as_str());
    let (status, then) = agent_send(&state, "GET", &format!("{agent}?as_of={d1_from}"), &token, None).await;
    assert_eq!(status, StatusCode::OK, "{then}");
    let ids: Vec<&str> = then["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [d1.as_str()], "as_of returns what was approved at that moment");
    for bad in [format!("as_of={d1_from}&status=all"), "as_of=yesterday".into(), "as_of=2026-13-45T00:00:00Z".into()] {
        let (status, _) = agent_send(&state, "GET", &format!("{agent}?{bad}"), &token, None).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    }

    let (status, _) = agent_send(&state, "POST", &format!("{agent}/{p1}/retire"), &token, Some("{}".into())).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a low key cannot retire");
    let (status, retired, _) = call(&state, "POST", &format!("{owner}/{p1}/retire"), Some(&cookie), Some(r#"{"note":"Pricing paused"}"#)).await;
    assert_eq!(status, StatusCode::OK, "{retired}");
    let retired: serde_json::Value = serde_json::from_str(&retired).unwrap();
    assert_eq!((retired["status"].as_str(), retired["invalidation_note"].as_str()), (Some("retired"), Some("Pricing paused")));
    let (status, body, _) = call(&state, "POST", &format!("{owner}/{p1}/retire"), Some(&cookie), Some("{}")).await;
    assert_eq!((status, body.contains("not_current")), (StatusCode::CONFLICT, true));
    let (_, current) = agent_send(&state, "GET", &agent, &token, None).await;
    assert_eq!(current["items"].as_array().unwrap().len(), 0, "retired items leave current recall");
    println!("PASS knowledge validity: disputed and conflicting items flagged, owner must resolve, replace supersedes with validity bounds, as_of recall, validation, owner-only retire once");
}

#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private PostgreSQL instance"]
async fn precheck_flow_against_live_database() {
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("AIDRIVE_LIVE_TEST_URL").unwrap())
        .await
        .unwrap();
    let state = AppState { db: Some(db.clone()), web_origin: "http://localhost:3000".into(), allow_signup: true };
    let mut cookies = Vec::new();
    for email in ["pre-owner@live.test", "pre-outsider@live.test"] {
        let creds = format!(r#"{{"email":"{email}","password":"correct horse battery staple"}}"#);
        let (_, _, cookie) = call(&state, "POST", "/auth/register", None, Some(&creds)).await;
        cookies.push(cookie.unwrap());
    }
    let (_, body, _) = call(&state, "POST", "/workspaces", Some(&cookies[0]), Some(r#"{"name":"Pre","review":"strict"}"#)).await;
    let ws = json_field(&body, "id");
    let mut tokens = Vec::new();
    for (n, scope) in [(111, "files:write"), (112, "files:read")] {
        let payload = format!(r#"{{"request_id":"{}","name":"k{n}","scope":"{scope}"}}"#, request_uuid(n));
        let (_, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/agents"), Some(&cookies[0]), Some(&payload)).await;
        tokens.push(serde_json::from_str::<serde_json::Value>(&body).unwrap()["token"].as_str().unwrap().to_owned());
    }
    let (token, reader) = (&tokens[0], &tokens[1]);
    let agent = format!("/agent/workspaces/{ws}");

    let (_, file) = agent_send(&state, "POST", &format!("{agent}/files"), token,
        Some(format!(r#"{{"request_id":"{}","name":"pricing.md","content":"Flat price"}}"#, request_uuid(113)))).await;
    let (file_id, v1) = (file["file_id"].as_str().unwrap().to_owned(), file["version_id"].as_str().unwrap().to_owned());
    let (_, session) = agent_send(&state, "POST", &format!("{agent}/sessions"), token,
        Some(r#"{"client":"codex","external_id":"pre1","label":"pricing work","origin":"import"}"#.into())).await;
    let sid = session["session_id"].as_str().unwrap().to_owned();
    agent_send(&state, "POST", &format!("{agent}/sessions/{sid}/events"), token, Some(
        r#"{"complete":true,"events":[{"key":"e0","ordinal":0,"role":"tool","at":"2026-01-01T00:00:00Z","text":"tool: Edit pricing.md"},
            {"key":"e1","ordinal":1,"role":"user","at":"2026-01-01T00:01:00Z","text":"pricing notes in md format"}]}"#.into())).await;

    let evidence = format!(r#"[{{"file_id":"{file_id}","version_id":"{v1}"}}]"#);
    let mut ids = Vec::new();
    for (n, kind, extra, statement) in [
        (114, "attempt", r#","outcome":"failed""#, "Per-seat pricing confused users"),
        (115, "decision", "", "Flat monthly price"),
        (116, "keystone", "", "Never change pricing.md without the owner"),
        (117, "fact", "", "Unrelated fact"),
    ] {
        let ev = if kind == "fact" { "[]" } else { evidence.as_str() };
        let (status, item) = agent_send(&state, "POST", &format!("{agent}/knowledge"), token, Some(format!(
            r#"{{"request_id":"{}","kind":"{kind}","topic":"pricing"{extra},"statement":"{statement}","evidence":{ev}}}"#, request_uuid(n)))).await;
        assert_eq!(status, StatusCode::CREATED, "{item}");
        ids.push(item["id"].as_str().unwrap().to_owned());
    }
    for item in &ids[..2] {
        let (status, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/knowledge/{item}/decision"), Some(&cookies[0]),
            Some(r#"{"decision":"approved"}"#)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let (status, handoff) = agent_send(&state, "POST", &format!("{agent}/handoffs"), token, Some(format!(
        r#"{{"request_id":"{}","summary":"Priced it","decisions":[],"next_step":"Check discounts","sources":[{{"file_id":"{file_id}","version_id":"{v1}"}}]}}"#,
        request_uuid(118)))).await;
    assert_eq!(status, StatusCode::CREATED, "{handoff}");
    let (_, v2) = agent_send(&state, "PUT", &format!("{agent}/files/{file_id}"), token,
        Some(format!(r#"{{"request_id":"{}","base_version_id":"{v1}","content":"Flat price, annual discount"}}"#, request_uuid(119)))).await;

    let (status, pre) = agent_send(&state, "GET", &format!("{agent}/files/{file_id}/precheck"), reader, None).await;
    assert_eq!(status, StatusCode::OK, "a files:read key can precheck: {pre}");
    assert_eq!((pre["file"]["revision"].as_i64(), pre["file"]["current_version_id"].as_str()), (Some(2), v2["version_id"].as_str()));
    let order: Vec<&str> = pre["knowledge"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap()).collect();
    assert_eq!(order, ["keystone", "attempt", "decision"], "rules, then failed attempts, then what relies on the file; unrelated items absent");
    assert!(pre["knowledge"].as_array().unwrap().iter().filter(|i| i["status"] == "approved").all(|i| i["stale"] == true),
        "approved items citing the old version are stale");
    assert_eq!((pre["handoffs"][0]["status"].as_str(), pre["handoffs"][0]["stale"].as_bool()), (Some("proposed"), Some(true)));
    let sessions = pre["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1, "{pre}");
    assert_eq!((sessions[0]["ordinal"].as_i64(), sessions[0]["changed_since"].as_bool()), (Some(0), Some(true)),
        "only the exact file name matches, and the file changed after that mention");
    assert!(sessions[0]["snippet"].as_str().unwrap().contains("«pricing.md»"), "{pre}");

    let (status, _, _) = call(&state, "GET", &format!("/workspaces/{ws}/files/{file_id}/precheck"), Some(&cookies[0]), None).await;
    assert_eq!(status, StatusCode::OK, "the owner can precheck from the web session");
    let (status, _, _) = call(&state, "GET", &format!("/workspaces/{ws}/files/{file_id}/precheck"), Some(&cookies[1]), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "outsiders learn nothing");
    let (status, _) = agent_send(&state, "GET", &format!("{agent}/files/{}/precheck", request_uuid(120)), reader, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    println!("PASS precheck: rules, failed attempts and dependent decisions citing the file in priority order, stale after edit, stale handoff, exact-name session mention with changed-since, read scope, outsider and unknown file 404");
}

#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private PostgreSQL instance"]
async fn visibility_flow_against_live_database() {
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("AIDRIVE_LIVE_TEST_URL").unwrap())
        .await
        .unwrap();
    let admin = sqlx::PgPool::connect(&std::env::var("AIDRIVE_FIXTURE_ADMIN_URL").unwrap()).await.unwrap();
    let state = AppState { db: Some(db.clone()), web_origin: "http://localhost:3000".into(), allow_signup: true };
    let (mut users, mut cookies) = (Vec::new(), Vec::new());
    for email in ["vis-owner@live.test", "vis-editor@live.test"] {
        let creds = format!(r#"{{"email":"{email}","password":"correct horse battery staple"}}"#);
        let (_, body, cookie) = call(&state, "POST", "/auth/register", None, Some(&creds)).await;
        users.push(json_field(&body, "id"));
        cookies.push(cookie.unwrap());
    }
    let (owner, editor) = (&cookies[0], &cookies[1]);
    let (_, body, _) = call(&state, "POST", "/workspaces", Some(owner), Some(r#"{"name":"Vis","review":"strict"}"#)).await;
    let ws = json_field(&body, "id");
    sqlx::query("INSERT INTO memberships (workspace_id, user_id, role) VALUES ($1::uuid, $2::uuid, 'editor')")
        .bind(&ws).bind(&users[1]).execute(&admin).await.unwrap();
    let payload = format!(r#"{{"request_id":"{}","name":"claude","scope":"files:write"}}"#, request_uuid(131));
    let (_, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/agents"), Some(owner), Some(&payload)).await;
    let token = serde_json::from_str::<serde_json::Value>(&body).unwrap()["token"].as_str().unwrap().to_owned();
    let (base, agent) = (format!("/workspaces/{ws}/sessions"), format!("/agent/workspaces/{ws}"));
    let get = |cookie: &str, path: String| {
        let (state, cookie) = (state.clone(), cookie.to_owned());
        async move {
            let (status, body, _) = call(&state, "GET", &path, Some(&cookie), None).await;
            (status, serde_json::from_str::<serde_json::Value>(&body).unwrap_or_default())
        }
    };

    let (_, session) = agent_send(&state, "POST", &format!("{agent}/sessions"), &token,
        Some(r#"{"client":"claude-code","external_id":"own-1","label":"owner work","origin":"import"}"#.into())).await;
    let s1 = session["session_id"].as_str().unwrap().to_owned();
    let (_, chat) = agent_send(&state, "POST", &format!("{agent}/chats"), &token,
        Some(r#"{"client":"claude","session_id":"own-1"}"#.into())).await;
    let transcript_url = format!("/workspaces/{ws}/chats/{}/transcript", chat["chat_id"].as_str().unwrap());
    let (status, _) = get(editor, transcript_url.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a workspace member cannot read another person's private chat capture");
    let (_, registry) = get(editor, format!("/workspaces/{ws}/chats")).await;
    assert!(registry["chats"][0]["title"].is_null(), "private capture titles must not leak through the chat picker");
    agent_send(&state, "POST", &format!("{agent}/sessions/{s1}/events"), &token,
        Some(r#"{"complete":true,"events":[{"key":"a","ordinal":0,"role":"user","text":"salary numbers for the hire"}]}"#.into())).await;
    let (_, mine) = get(owner, base.clone()).await;
    assert_eq!((mine[0]["visibility"].as_str(), mine[0]["mine"].as_bool()), (Some("private"), Some(true)), "{mine}");
    let (_, theirs) = get(editor, base.clone()).await;
    assert_eq!(theirs.as_array().map(Vec::len), Some(0), "another member does not see a private session");
    let (_, hits) = get(editor, format!("{base}/search?q=salary")).await;
    assert_eq!(hits.as_array().map(Vec::len), Some(0), "nor finds it in search");

    let (status, _) = agent_send(&state, "POST", &format!("{agent}/sessions/{s1}/visibility"), &token, Some(r#"{"visibility":"team"}"#.into())).await;
    assert!(matches!(status, StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED), "assistants have no sharing route");
    let (status, _, _) = call(&state, "POST", &format!("{base}/{s1}/visibility"), Some(editor), Some(r#"{"visibility":"team"}"#)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "only the creator can share");
    let (status, _, _) = call(&state, "POST", &format!("{base}/{s1}/visibility"), Some(owner), Some(r#"{"visibility":"public"}"#)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _, _) = call(&state, "POST", &format!("{base}/{s1}/visibility"), Some(owner), Some(r#"{"visibility":"team"}"#)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, shared) = get(editor, transcript_url).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(shared["events"][0]["text"], "salary numbers for the hire", "sharing makes the exact captured conversation visible");
    let (_, registry) = get(editor, format!("/workspaces/{ws}/chats")).await;
    assert_eq!(registry["chats"][0]["title"], "owner work");
    let (_, theirs) = get(editor, base.clone()).await;
    assert_eq!((theirs[0]["visibility"].as_str(), theirs[0]["mine"].as_bool()), (Some("team"), Some(false)), "{theirs}");
    let (_, hits) = get(editor, format!("{base}/search?q=salary")).await;
    assert_eq!(hits.as_array().map(Vec::len), Some(1), "a shared session is searchable by the team");

    let (status, body, _) = call(&state, "POST", &base, Some(editor),
        Some(r#"{"client":"claude-code","external_id":"own-1","label":"hijack","origin":"import"}"#)).await;
    assert_eq!((status, body.contains("session_belongs_to_another_person")), (StatusCode::CONFLICT, true), "{body}");
    let (status, body, _) = call(&state, "POST", &base, Some(editor),
        Some(r#"{"client":"codex","external_id":"ed-1","label":"editor draft","origin":"import"}"#)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let s2 = json_field(&body, "session_id");
    let (_, mine) = get(owner, base.clone()).await;
    assert_eq!(mine.as_array().map(Vec::len), Some(1), "the workspace owner does not see the editor's private session");

    let k = format!("/workspaces/{ws}/knowledge");
    let proposal = format!(r#"{{"request_id":"{}","kind":"preference","topic":"tone","statement":"I want terse replies","visibility":"private"}}"#, request_uuid(132));
    let (status, item, _) = call(&state, "POST", &k, Some(editor), Some(&proposal)).await;
    assert_eq!(status, StatusCode::CREATED, "{item}");
    let item: serde_json::Value = serde_json::from_str(&item).unwrap();
    assert_eq!((item["visibility"].as_str(), item["mine"].as_bool()), (Some("private"), Some(true)));
    let id = item["id"].as_str().unwrap().to_owned();
    let (_, inbox) = get(owner, format!("{k}?status=proposed")).await;
    assert_eq!(inbox["items"].as_array().map(Vec::len), Some(0), "private proposals are not in the owner's inbox");
    let (status, _, _) = call(&state, "POST", &format!("{k}/{id}/decision"), Some(owner), Some(r#"{"decision":"approved"}"#)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the owner cannot decide someone's private item");
    let (status, decided, _) = call(&state, "POST", &format!("{k}/{id}/decision"), Some(editor), Some(r#"{"decision":"approved"}"#)).await;
    assert_eq!(status, StatusCode::OK, "{decided}");
    let (_, seen) = agent_send(&state, "GET", &format!("{agent}/knowledge"), &token, None).await;
    assert_eq!(seen["items"].as_array().map(Vec::len), Some(0), "the owner's assistants do not see the editor's private knowledge");
    let (status, _, _) = call(&state, "POST", &k, Some(editor), Some(&proposal.replace("private", "secret"))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _, _) = call(&state, "DELETE", &format!("{base}/{s2}"), Some(editor), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "the creator deletes their own session");
    println!("PASS visibility: sessions private by default, hidden from other members and search, creator-only sharing, no merge into another person's session, private knowledge outside the owner's inbox and decided by its creator, assistants see only their person's private items, creator delete");
}
