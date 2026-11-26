#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private PostgreSQL instance"]
async fn roles_flow_against_live_database() {
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("AIDRIVE_LIVE_TEST_URL").unwrap())
        .await
        .unwrap();
    let state = AppState { db: Some(db.clone()), web_origin: "http://localhost:3000".into(), allow_signup: true };
    let creds = r#"{"email":"roles-owner@live.test","password":"correct horse battery staple"}"#;
    let (_, _, cookie) = call(&state, "POST", "/auth/register", None, Some(creds)).await;
    let owner = cookie.unwrap();
    let (_, body, _) = call(&state, "POST", "/workspaces", Some(&owner), Some(r#"{"name":"Roles API"}"#)).await;
    let ws = json_field(&body, "id");
    let (status, _, _) = call(&state, "POST", &format!("/workspaces/{ws}/review"), Some(&owner), Some(r#"{"mode":"strict"}"#)).await;
    assert_eq!(status, StatusCode::OK);
    let mut keys = Vec::new();
    for (n, name, scope) in [(171, "low", "files:write"), (172, "medium", "review:accept"), (173, "grand", "grand")] {
        let payload = format!(r#"{{"request_id":"{}","name":"{name}","scope":"{scope}"}}"#, request_uuid(n));
        let (status, body, _) = call(&state, "POST", &format!("/workspaces/{ws}/agents"), Some(&owner), Some(&payload)).await;
        assert_eq!(status, StatusCode::CREATED, "{scope}: {body}");
        keys.push(serde_json::from_str::<serde_json::Value>(&body).unwrap()["token"].as_str().unwrap().to_owned());
    }
    let (low, medium, grand) = (&keys[0], &keys[1], &keys[2]);
    let agent = format!("/agent/workspaces/{ws}");
    let propose = |n: u32, statement: &str| Some(format!(r#"{{"request_id":"{}","kind":"fact","topic":"t{n}","statement":"{statement}"}}"#, request_uuid(n)));

    let (status, p1) = agent_send(&state, "POST", &format!("{agent}/knowledge"), low, propose(174, "We ship weekly")).await;
    assert_eq!((status, p1["status"].as_str()), (StatusCode::CREATED, Some("proposed")), "{p1}");
    let p1 = p1["id"].as_str().unwrap().to_owned();
    let (_, p2) = agent_send(&state, "POST", &format!("{agent}/knowledge"), medium, propose(175, "Flat price")).await;
    let p2 = p2["id"].as_str().unwrap().to_owned();
    let approve = Some(r#"{"decision":"approved"}"#.to_owned());

    let (status, _) = agent_send(&state, "POST", &format!("{agent}/knowledge/{p1}/decision"), low, approve.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "low cannot approve");
    let (status, _) = agent_send(&state, "POST", &format!("{agent}/knowledge/{p2}/decision"), medium, approve.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "medium cannot approve its own proposal");
    let (status, item) = agent_send(&state, "POST", &format!("{agent}/knowledge/{p1}/decision"), medium, approve.clone()).await;
    assert_eq!((status, item["status"].as_str()), (StatusCode::OK, Some("approved")), "{item}");
    let (status, _) = agent_send(&state, "POST", &format!("{agent}/knowledge/{p1}/retire"), medium, Some("{}".into())).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "medium cannot retire");
    let (status, _) = agent_send(&state, "POST", &format!("{agent}/review"), medium, Some(r#"{"mode":"exceptions"}"#.into())).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "medium cannot change the review mode");

    let (status, item) = agent_send(&state, "POST", &format!("{agent}/knowledge/{p2}/decision"), grand, approve.clone()).await;
    assert_eq!((status, item["status"].as_str()), (StatusCode::OK, Some("approved")), "{item}");
    let (status, item) = agent_send(&state, "POST", &format!("{agent}/knowledge/{p1}/retire"), grand, Some(r#"{"note":"cadence changed"}"#.into())).await;
    assert_eq!((status, item["status"].as_str()), (StatusCode::OK, Some("retired")), "{item}");

    let (_, activity, _) = call(&state, "GET", &format!("/workspaces/{ws}/activity?limit=50"), Some(&owner), None).await;
    let activity: serde_json::Value = serde_json::from_str(&activity).unwrap();
    let by = |kind: &str| -> Vec<String> {
        activity["events"].as_array().unwrap().iter()
            .filter(|e| e["kind"] == kind)
            .filter_map(|e| e["agent_name"].as_str().map(str::to_owned))
            .collect()
    };
    assert!(by("knowledge.approved").contains(&"medium".to_owned()) && by("knowledge.approved").contains(&"grand".to_owned()),
        "the timeline names the deciding key: {:?}", by("knowledge.approved"));
    assert_eq!(by("knowledge.retired"), vec!["grand".to_owned()]);

    let bad = format!(r#"{{"request_id":"{}","name":"x","scope":"root"}}"#, request_uuid(176));
    let (status, _, _) = call(&state, "POST", &format!("/workspaces/{ws}/agents"), Some(&owner), Some(&bad)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "unknown roles are refused");
    println!("PASS roles: owner issues low/medium/grand keys; low cannot decide; medium approves others' proposals but not its own, cannot retire or change the mode; grand accepts its own and retires; the timeline names the deciding key");
}
