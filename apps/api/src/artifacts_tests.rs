#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private PostgreSQL instance"]
async fn file_index_flow_against_live_database() {
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("AIDRIVE_LIVE_TEST_URL").unwrap())
        .await
        .unwrap();
    let state = AppState { db: Some(db.clone()), web_origin: "http://localhost:3000".into(), allow_signup: true };
    let mut cookies = Vec::new();
    for email in ["idx-owner@live.test", "idx-outsider@live.test"] {
        let creds = format!(r#"{{"email":"{email}","password":"correct horse battery staple"}}"#);
        let (_, _, cookie) = call(&state, "POST", "/auth/register", None, Some(&creds)).await;
        cookies.push(cookie.unwrap());
    }
    let owner = cookies[0].clone();
    let (_, body, _) = call(&state, "POST", "/workspaces", Some(&owner), Some(r#"{"name":"Idx"}"#)).await;
    let ws = json_field(&body, "id");
    let mut keys = Vec::new();
    for (n, name, scope) in [(161, "claude", "files:write"), (162, "reader", "files:read")] {
        let payload = format!(r#"{{"request_id":"{}","name":"{name}","scope":"{scope}"}}"#, request_uuid(n));
        let (_, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/agents"), Some(&owner), Some(&payload)).await;
        keys.push(serde_json::from_str::<serde_json::Value>(&body).unwrap()["token"].as_str().unwrap().to_owned());
    }
    let (claude, reader) = (&keys[0], &keys[1]);
    let agent = format!("/agent/workspaces/{ws}");

    let (status, a) = agent_send(&state, "POST", &format!("{agent}/artifacts"), claude, Some(
        r#"{"path":"C:\\work\\probe.py","host":"pc","worktree":"C:\\work","branch":"claude/x","purpose":"probe","keep_until":"2020-01-01T00:00:00Z"}"#.into())).await;
    assert_eq!(status, StatusCode::CREATED, "{a}");
    assert_eq!((a["due"].as_bool(), a["registered_by_agent_name"].as_str(), a["branch"].as_str()), (Some(true), Some("claude"), Some("claude/x")));
    let a_id = a["id"].as_str().unwrap().to_owned();
    let (status, b) = agent_send(&state, "POST", &format!("{agent}/artifacts"), claude,
        Some(r#"{"path":"C:\\work\\notes.md","host":"pc","purpose":"notes","keep_days":3}"#.into())).await;
    assert_eq!((status, b["due"].as_bool()), (StatusCode::CREATED, Some(false)), "{b}");
    let b_id = b["id"].as_str().unwrap().to_owned();

    let (_, due) = agent_send(&state, "GET", &format!("{agent}/artifacts?due=true&host=pc"), reader, None).await;
    assert_eq!(due["artifacts"].as_array().unwrap().len(), 1, "readers list; only the past-due file is due");
    let (status, _) = agent_send(&state, "POST", &format!("{agent}/artifacts"), reader,
        Some(r#"{"path":"x","purpose":"x"}"#.into())).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a files:read key cannot register");

    let (_, checked) = agent_send(&state, "POST", &format!("{agent}/artifacts/check"), claude,
        Some(format!(r#"{{"checks":[{{"id":"{a_id}","exists":true}},{{"id":"{b_id}","exists":false}}]}}"#))).await;
    assert_eq!(checked["updated"], 2);
    let (_, r) = agent_send(&state, "POST", &format!("{agent}/artifacts/{a_id}/resolve"), claude,
        Some(r#"{"action":"removed","note":"deleted after the fix"}"#.into())).await;
    assert_eq!(r["status"], "removed");
    let (_, all, _) = call(&state, "GET", &format!("/workspaces/{ws}/artifacts?status=all"), Some(&owner), None).await;
    let all: serde_json::Value = serde_json::from_str(&all).unwrap();
    let statuses: Vec<&str> = all["artifacts"].as_array().unwrap().iter().map(|x| x["status"].as_str().unwrap()).collect();
    assert_eq!(statuses.len(), 2);
    assert!(statuses.contains(&"removed") && statuses.contains(&"missing"), "{statuses:?}");

    let build = |size: u64| Some(format!(r#"{{"path":"C:/work/target","host":"pc","purpose":"Rust build output (rebuildable)","keep_days":7,"kind":"build","size_bytes":{size},"session":"s-1","refresh":true}}"#));
    let (status, t1) = agent_send(&state, "POST", &format!("{agent}/artifacts"), claude, build(5_000_000_000)).await;
    assert_eq!((status, t1["kind"].as_str(), t1["size_bytes"].as_i64()), (StatusCode::CREATED, Some("build"), Some(5_000_000_000)), "{t1}");
    let (_, t2) = agent_send(&state, "POST", &format!("{agent}/artifacts"), claude, build(7_000_000_000)).await;
    assert_eq!((t2["id"].clone(), t2["keep_until"].clone(), t2["size_bytes"].as_i64()), (t1["id"].clone(), t1["keep_until"].clone(), Some(7_000_000_000)),
        "a re-index updates the size only");
    for bad in [r#"{"path":"x","purpose":"x","kind":"cache"}"#, r#"{"path":"x","purpose":"x","size_bytes":-1}"#] {
        let (status, body) = agent_send(&state, "POST", &format!("{agent}/artifacts"), claude, Some(bad.into())).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}: {body}");
    }
    for bad in [r#"{"path":"  ","purpose":"x"}"#, r#"{"path":"x","purpose":"x","keep_days":0}"#,
                r#"{"path":"x","purpose":"x","keep_until":"tomorrow"}"#, r#"{"path":"x","purpose":"x","keep_days":1,"keep_until":"2030-01-01T00:00:00Z"}"#] {
        let (status, body) = agent_send(&state, "POST", &format!("{agent}/artifacts"), claude, Some(bad.into())).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}: {body}");
    }
    let (status, _) = agent_send(&state, "POST", &format!("{agent}/artifacts/{b_id}/resolve"), claude,
        Some(r#"{"action":"extend"}"#.into())).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "extend needs a date");
    let (status, _, _) = call(&state, "GET", &format!("/workspaces/{ws}/artifacts"), Some(&cookies[1]), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "outsiders see nothing");
    println!("PASS file index: assistants register files with worktree, branch and keep_until; due list per machine; checks mark missing; keep/extend/removed; build folders with kind and size, re-index updates size only; readers list but cannot register; validation; outsider 404");
}
