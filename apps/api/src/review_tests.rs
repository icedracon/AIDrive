#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private PostgreSQL instance"]
async fn review_by_exception_flow_against_live_database() {
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
    for email in ["review-owner@live.test", "review-outsider@live.test"] {
        let creds = format!(r#"{{"email":"{email}","password":"correct horse battery staple"}}"#);
        let (_, _, cookie) = call(&state, "POST", "/auth/register", None, Some(&creds)).await;
        cookies.push(cookie.unwrap());
    }
    let owner = &cookies[0];
    let (status, _, _) = call(&state, "POST", "/workspaces", Some(owner), Some(r#"{"name":"Bad","review":"lenient"}"#)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (_, body, _) = call(&state, "POST", "/workspaces", Some(owner), Some(r#"{"name":"Review"}"#)).await;
    let ws = json_field(&body, "id");
    let (_, body, _) = call(&state, "GET", &format!("/workspaces/{ws}/review"), Some(owner), None).await;
    let review: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(review["mode"], "exceptions", "new spaces review by exception");
    let payload = format!(r#"{{"request_id":"{}","name":"claude","scope":"files:write"}}"#, request_uuid(201));
    let (_, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/agents"), Some(owner), Some(&payload)).await;
    let token = serde_json::from_str::<serde_json::Value>(&body).unwrap()["token"].as_str().unwrap().to_owned();
    let agent = format!("/agent/workspaces/{ws}");

    let (_, file) = agent_send(&state, "POST", &format!("{agent}/files"), &token,
        Some(format!(r#"{{"request_id":"{}","name":"plan.md","content":"Ship weekly"}}"#, request_uuid(202)))).await;
    let (file_id, v1) = (file["file_id"].as_str().unwrap().to_owned(), file["version_id"].as_str().unwrap().to_owned());
    let propose = |n: u32, kind: &str, topic: &str, statement: &str, evidence: bool| {
        let evidence = if evidence { format!(r#"[{{"file_id":"{file_id}","version_id":"{v1}"}}]"#) } else { "[]".into() };
        Some(format!(
            r#"{{"request_id":"{}","kind":"{kind}","topic":"{topic}","statement":"{statement}","evidence":{evidence}}}"#,
            request_uuid(n)
        ))
    };
    let (status, clean) = agent_send(&state, "POST", &format!("{agent}/knowledge"), &token, propose(203, "fact", "cadence", "We ship weekly", true)).await;
    assert_eq!(status, StatusCode::CREATED, "{clean}");
    assert_eq!((clean["status"].as_str(), clean["auto_accepted"].as_bool(), clean["owner_checked"].as_bool()),
        (Some("approved"), Some(true), Some(false)));
    let (_, conflict) = agent_send(&state, "POST", &format!("{agent}/knowledge"), &token, propose(204, "fact", "cadence", "We ship monthly", true)).await;
    assert_eq!(conflict["status"], "proposed");
    assert_eq!(conflict["review_reasons"], serde_json::json!(["conflict"]));
    let (_, keystone) = agent_send(&state, "POST", &format!("{agent}/knowledge"), &token, propose(205, "keystone", "rules", "Never push", false)).await;
    assert_eq!(keystone["review_reasons"], serde_json::json!(["keystone", "no_evidence"]));
    let (_, current) = agent_send(&state, "GET", &format!("{agent}/knowledge?q=weekly"), &token, None).await;
    assert_eq!(current["items"][0]["auto_accepted"], true, "recall tells assistants it was not owner-checked");
    println!("PASS review: new spaces default to exceptions; clean proposals auto-accepted and labelled; conflicts and keystones wait with reasons");

    let (_, handoff) = agent_send(&state, "POST", &format!("{agent}/handoffs"), &token, Some(format!(
        r#"{{"request_id":"{}","summary":"Drafted the plan","decisions":["Weekly demos"],"next_step":"Script","sources":[{{"file_id":"{file_id}","version_id":"{v1}"}}]}}"#,
        request_uuid(206)))).await;
    assert_eq!((handoff["status"].as_str(), handoff["auto_accepted"].as_bool()), (Some("approved"), Some(true)));
    let (_, bare) = agent_send(&state, "POST", &format!("{agent}/handoffs"), &token, Some(format!(
        r#"{{"request_id":"{}","summary":"No sources","next_step":"Ask"}}"#, request_uuid(207)))).await;
    assert_eq!(bare["review_reasons"], serde_json::json!(["no_sources"]));
    let (_, latest) = agent_send(&state, "GET", &format!("{agent}/handoffs/latest"), &token, None).await;
    assert_eq!(latest["approved"]["id"], handoff["id"]);

    let (status, review) = agent_send(&state, "GET", &format!("{agent}/review"), &token, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(review["recent_knowledge"].as_array().unwrap().len(), 1, "derived handoff decisions are listed with their handoff");
    assert_eq!(review["recent_handoffs"][0]["id"], handoff["id"]);

    let (status, _) = agent_send(&state, "POST", &format!("{agent}/review"), &token, Some(r#"{"mode":"strict"}"#.into())).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a low key cannot change the review mode");
    let clean_id = clean["id"].as_str().unwrap();
    let (status, _) = agent_send(&state, "POST", &format!("{agent}/knowledge/{clean_id}/undo"), &token, Some("{}".into())).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a low key cannot undo");
    let (status, _, _) = call(&state, "POST", &format!("/workspaces/{ws}/knowledge/{clean_id}/undo"), Some(&cookies[1]), Some("{}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "outsiders cannot undo");

    let (status, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/knowledge/{clean_id}/undo"), Some(owner), Some(r#"{"note":"Wrong"}"#)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let undone: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!((undone["status"].as_str(), undone["invalidation_note"].as_str()), (Some("retired"), Some("Wrong")));
    let (status, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/knowledge/{clean_id}/undo"), Some(owner), Some("{}")).await;
    assert_eq!((status, body.as_str()), (StatusCode::CONFLICT, r#"{"error":"not_auto_accepted"}"#));
    let handoff_id = handoff["id"].as_str().unwrap();
    let (status, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/handoffs/{handoff_id}/keep"), Some(owner), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let kept: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(kept["owner_checked"], true);
    let (_, body, _) = call(&state, "GET", &format!("/workspaces/{ws}/review"), Some(owner), None).await;
    let review: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(review["recent_knowledge"].as_array().unwrap().is_empty() && review["recent_handoffs"].as_array().unwrap().is_empty());
    println!("PASS review: owner undo retires once, keep marks owner-checked, assistants and outsiders cannot undo or change the mode");

    let (status, metrics) = agent_send(&state, "GET", &format!("{agent}/review/metrics?days=3"), &token, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(metrics["days"].as_array().unwrap().len(), 3);
    assert_eq!((metrics["totals"]["auto_accepted"].as_i64(), metrics["totals"]["undone"].as_i64(), metrics["totals"]["kept"].as_i64()),
        (Some(2), Some(1), Some(1)));
    assert_eq!(metrics["undo_rate"], 0.5);
    let (status, _) = agent_send(&state, "GET", &format!("{agent}/review/metrics?days=0"), &token, None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (_, body, _) = call(&state, "GET", &format!("/workspaces/{ws}/activity"), Some(owner), None).await;
    for kind in ["knowledge.auto_accepted", "knowledge.undone", "handoff.auto_accepted", "handoff.kept"] {
        assert!(body.contains(kind), "timeline has {kind}");
    }
    println!("PASS review: metrics per day with undo rate, timeline shows auto-acceptance, keep and undo");

    let (status, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/review"), Some(owner), Some(r#"{"mode":"strict"}"#)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, strict) = agent_send(&state, "POST", &format!("{agent}/knowledge"), &token, propose(208, "fact", "hosting", "One VM", true)).await;
    assert_eq!((strict["status"].as_str(), strict["review_reasons"].clone()), (Some("proposed"), serde_json::json!(["strict_review"])));
    let (status, _, _) = call(&state, "POST", &format!("/workspaces/{ws}/review"), Some(owner), Some(r#"{"mode":"off"}"#)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    println!("PASS review: the owner switches back to strict review and proposals wait again");
}
