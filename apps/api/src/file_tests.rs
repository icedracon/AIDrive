async fn file_call(
    state: &AppState,
    method: &str,
    url: &str,
    cookie: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let body = body.map(|v| v.to_string());
    let (status, result, _) = call(state, method, url, Some(cookie), body.as_deref()).await;
    (status, serde_json::from_str(&result).expect(&result))
}

fn file_request(n: usize) -> String {
    format!("60000000-0000-4000-8000-{n:012}")
}

#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private PostgreSQL instance"]
async fn files_flow_against_live_database() {
    use serde_json::json;
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&std::env::var("AIDRIVE_LIVE_TEST_URL").unwrap())
        .await
        .unwrap();
    let admin = PgPool::connect(&std::env::var("AIDRIVE_FIXTURE_ADMIN_URL").unwrap())
        .await
        .unwrap();
    let state = AppState {
        db: Some(db.clone()),
        web_origin: "http://localhost:3000".into(),
        allow_signup: true,
    };
    let mut cookies = Vec::new();
    let mut users = Vec::new();
    for email in [
        "files-owner@live.test",
        "files-outsider@live.test",
        "files-viewer@live.test",
    ] {
        let body = json!({"email":email,"password":"correct horse battery staple"}).to_string();
        let (status, result, cookie) =
            call(&state, "POST", "/auth/register", None, Some(&body)).await;
        assert_eq!(status, StatusCode::CREATED);
        users.push(json_field(&result, "id"));
        cookies.push(cookie.unwrap());
    }
    let (_, ws) = file_call(
        &state,
        "POST",
        "/workspaces",
        &cookies[0],
        Some(json!({"name":"Files acceptance"})),
    )
    .await;
    let workspace = ws["workspace"]["id"].as_str().unwrap();
    sqlx::query(
        "INSERT INTO memberships(workspace_id,user_id,role) VALUES ($1::uuid,$2::uuid,'viewer')",
    )
    .bind(workspace)
    .bind(&users[2])
    .execute(&admin)
    .await
    .unwrap();
    let endpoint = format!("/workspaces/{workspace}/files");
    let content = "Բարեւ աշխարհ\r\n<script>alert('untrusted')</script>\n";
    let create = json!({"request_id":file_request(1),"name":"Նշում.md","content":content});
    let (status, original) =
        file_call(&state, "POST", &endpoint, &cookies[0], Some(create.clone())).await;
    assert_eq!(status, StatusCode::CREATED, "{original}");
    assert_eq!(original["content"], content);
    assert_eq!(original["revision"], 1);
    assert_eq!(original["actor_id"], users[0]);
    let file = original["file_id"].as_str().unwrap();
    let first = original["version_id"].as_str().unwrap();
    let url = format!("{endpoint}/{file}");
    let history = format!("{url}/versions");
    let retry = file_call(&state, "POST", &endpoint, &cookies[0], Some(create.clone())).await;
    assert_eq!(retry.0, StatusCode::CREATED);
    assert_eq!(retry.1["version_id"], first);
    let mut changed = create.clone();
    changed["content"] = json!("changed");
    assert_eq!(
        file_call(&state, "POST", &endpoint, &cookies[0], Some(changed))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let duplicate = file_call(
        &state,
        "POST",
        &endpoint,
        &cookies[0],
        Some(json!({"request_id":"60000000-0000-4000-8000-0000000000d1","name":"Նշում.MD","content":"second copy"})),
    )
    .await;
    assert_eq!(duplicate.0, StatusCode::CONFLICT, "{}", duplicate.1);
    assert_eq!(duplicate.1["error"], "name_taken");
    assert_eq!(
        file_call(&state, "GET", &endpoint, &cookies[0], None)
            .await
            .1["files"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        file_call(&state, "GET", &url, &cookies[2], None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        file_call(&state, "POST", &endpoint, &cookies[2], Some(create.clone()))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    for target in [&endpoint, &url, &history, &format!("{history}/{first}")] {
        assert_eq!(
            file_call(&state, "GET", target, &cookies[1], None).await.0,
            StatusCode::NOT_FOUND
        );
    }
    let (_, second_ws) = file_call(
        &state,
        "POST",
        "/workspaces",
        &cookies[0],
        Some(json!({"name":"Other owned space"})),
    )
    .await;
    let other_ws = second_ws["workspace"]["id"].as_str().unwrap();
    assert_eq!(
        file_call(
            &state,
            "GET",
            &format!("/workspaces/{other_ws}/files/{file}"),
            &cookies[0],
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    for name in [
        "../secrets.txt",
        "C:\\data.txt",
        "index.html",
        "invalid\n.txt",
    ] {
        assert_eq!(
            file_call(
                &state,
                "POST",
                &endpoint,
                &cookies[0],
                Some(json!({"request_id":file_request(2),"name":name,"content":"x"}))
            )
            .await
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    assert_eq!(file_call(&state,"POST",&endpoint,&cookies[0],Some(json!({"request_id":file_request(2),"name":"large.txt","content":"ա".repeat(131073)}))).await.0,StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        file_call(
            &state,
            "POST",
            &endpoint,
            &cookies[0],
            Some(json!({"request_id":file_request(2),"name":"nul.txt","content":"x\0y"}))
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let response = app(state.clone())
        .oneshot(
            Request::builder()
                .uri(format!("{history}/{first}/download"))
                .header("cookie", &cookies[0])
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(
        response.headers()["content-type"],
        "text/plain; charset=utf-8"
    );
    assert!(
        response.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment;")
    );
    assert_eq!(
        axum::body::to_bytes(response.into_body(), 1048576)
            .await
            .unwrap()
            .as_ref(),
        content.as_bytes()
    );
    println!(
        "PASS text create/read/download, exact UTF-8 bytes, retry deduplication, workspace/viewer isolation and upload validation"
    );

    let mut held = admin.begin().await.unwrap();
    sqlx::query("SELECT id FROM files WHERE id=$1::uuid FOR UPDATE")
        .bind(file)
        .execute(&mut *held)
        .await
        .unwrap();
    let payload_a =
        json!({"request_id":file_request(10),"base_version_id":first,"content":"Writer A"});
    let payload_b =
        json!({"request_id":file_request(11),"base_version_id":first,"content":"Writer B"});
    let spawn_write = |payload| {
        let state = state.clone();
        let url = url.clone();
        let cookie = cookies[0].clone();
        tokio::spawn(async move { file_call(&state, "PUT", &url, &cookie, Some(payload)).await })
    };
    let a = spawn_write(payload_a.clone());
    let b = spawn_write(payload_b.clone());
    tokio::time::timeout(Duration::from_secs(8),async {
        loop {
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE usename='aidrive_app' AND wait_event_type='Lock' AND query LIKE '%app.write_text_file%'").fetch_one(&admin).await.unwrap();
            if count >= 2 { break; }
            assert!(!a.is_finished() && !b.is_finished());
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("both writers must overlap on the file row lock");
    held.commit().await.unwrap();
    let a = a.await.unwrap();
    let b = b.await.unwrap();
    let (winner, winner_payload) = if a.0 == StatusCode::OK {
        assert_eq!(b.0, StatusCode::CONFLICT);
        (a.1, payload_a)
    } else {
        assert_eq!(a.0, StatusCode::CONFLICT);
        assert_eq!(b.0, StatusCode::OK);
        (b.1, payload_b)
    };
    assert_eq!(winner["revision"], 2);
    assert_eq!(
        file_call(
            &state,
            "GET",
            &format!("{history}/{first}"),
            &cookies[0],
            None
        )
        .await
        .1["content"],
        content
    );
    let head = winner["version_id"].as_str().unwrap();
    let restore =
        json!({"request_id":file_request(12),"base_version_id":head,"source_version_id":first});
    let (status, restored) = file_call(
        &state,
        "POST",
        &format!("{url}/restore"),
        &cookies[0],
        Some(restore.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert_eq!(restored["content"], content);
    assert_eq!(restored["revision"], 3);
    assert_eq!(restored["parent_version_id"], head);
    assert_eq!(restored["restored_from"], first);
    assert_eq!(restored["sha256"], original["sha256"]);
    assert_eq!(
        file_call(
            &state,
            "POST",
            &format!("{url}/restore"),
            &cookies[0],
            Some(restore)
        )
        .await
        .1["version_id"],
        restored["version_id"]
    );
    assert_eq!(
        file_call(&state, "PUT", &url, &cookies[0], Some(winner_payload))
            .await
            .1["version_id"],
        head
    );
    assert_eq!(
        file_call(&state, "GET", &url, &cookies[0], None).await.1["version_id"],
        restored["version_id"]
    );
    assert_eq!(
        file_call(&state, "GET", &history, &cookies[0], None)
            .await
            .1["versions"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    println!(
        "PASS overlapping conditional writes have one winner; history immutable; restore creates a new version; old retry never rewinds head"
    );

    let mut scoped = crate::workspaces::scoped(&db, &users[0], Some(workspace))
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE files SET current_version_id=NULL WHERE id=$1::uuid")
            .bind(file)
            .execute(&mut *scoped)
            .await
            .is_err()
    );
    scoped.rollback().await.unwrap();
    let mut scoped = crate::workspaces::scoped(&db, &users[0], Some(workspace))
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE file_versions SET content='tampered' WHERE id=$1::uuid")
            .bind(first)
            .execute(&mut *scoped)
            .await
            .is_err()
    );
    scoped.rollback().await.unwrap();
    let counts: (i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM file_versions WHERE file_id=$1::uuid),(SELECT count(*) FROM file_actions WHERE file_id=$1::uuid)").bind(file).fetch_one(&admin).await.unwrap();
    assert_eq!(counts, (3, 3));
    sqlx::raw_sql("CREATE FUNCTION public.fail_file_action_fixture() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected action failure'; END $$; CREATE TRIGGER fail_file_action_fixture BEFORE INSERT ON file_actions FOR EACH ROW EXECUTE FUNCTION public.fail_file_action_fixture();").execute(&admin).await.unwrap();
    let failing = json!({"request_id":file_request(20),"base_version_id":restored["version_id"],"content":"After recovered write"});
    assert_eq!(
        file_call(&state, "PUT", &url, &cookies[0], Some(failing.clone()))
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let counts: (i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM file_versions WHERE file_id=$1::uuid),(SELECT count(*) FROM file_actions WHERE file_id=$1::uuid)").bind(file).fetch_one(&admin).await.unwrap();
    assert_eq!(counts, (3, 3));
    assert_eq!(
        file_call(&state, "GET", &url, &cookies[0], None).await.1["version_id"],
        restored["version_id"]
    );
    sqlx::raw_sql("DROP TRIGGER fail_file_action_fixture ON file_actions; DROP FUNCTION public.fail_file_action_fixture();").execute(&admin).await.unwrap();
    let (status, recovered) = file_call(&state, "PUT", &url, &cookies[0], Some(failing)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recovered["revision"], 4);
    println!(
        "PASS direct version/head tampering denied; injected action failure rolls back bytes, head and event; retry succeeds once"
    );

    let (_, other) = file_call(
        &state,
        "POST",
        &endpoint,
        &cookies[0],
        Some(json!({"request_id":file_request(21),"name":"other.txt","content":""})),
    )
    .await;
    assert_eq!(file_call(&state,"POST",&format!("{url}/restore"),&cookies[0],Some(json!({"request_id":file_request(22),"base_version_id":recovered["version_id"],"source_version_id":other["version_id"]}))).await.0,StatusCode::NOT_FOUND);
    sqlx::query(
        "UPDATE memberships SET role='editor' WHERE workspace_id=$1::uuid AND user_id=$2::uuid",
    )
    .bind(workspace)
    .bind(&users[2])
    .execute(&admin)
    .await
    .unwrap();
    let (status,edited)=file_call(&state,"PUT",&url,&cookies[2],Some(json!({"request_id":file_request(23),"base_version_id":recovered["version_id"],"content":content}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(edited["actor_id"], users[2]);
    assert_eq!(edited["created_by"], users[0]);
    assert_eq!(edited["revision"], 5);
    let fixture = std::path::PathBuf::from(std::env::var("AIDRIVE_TEST_CLUSTER").unwrap());
    assert!(fixture.join(".aidrive-private").is_file());
    std::fs::write(
        fixture.join("file-restart.json"),
        json!({"cookie":cookies[0],"url":url,"version":edited}).to_string(),
    )
    .unwrap();
    db.close().await;
    admin.close().await;
    println!(
        "PASS cross-file restore denied and editor attribution preserved; restart fixture recorded privately"
    );
}

#[tokio::test]
#[ignore = "scripts/api-check.sh restarts its private database before this check"]
async fn files_survive_database_restart() {
    let fixture = std::path::PathBuf::from(std::env::var("AIDRIVE_TEST_CLUSTER").unwrap());
    assert!(fixture.join(".aidrive-private").is_file());
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fixture.join("file-restart.json")).unwrap())
            .unwrap();
    let db = PgPool::connect(&std::env::var("AIDRIVE_LIVE_TEST_URL").unwrap())
        .await
        .unwrap();
    let state = AppState {
        db: Some(db.clone()),
        web_origin: "http://localhost:3000".into(),
        allow_signup: false,
    };
    let (status, result) = file_call(
        &state,
        "GET",
        saved["url"].as_str().unwrap(),
        saved["cookie"].as_str().unwrap(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result, saved["version"]);
    db.close().await;
    println!(
        "PASS real PostgreSQL restart plus fresh API process preserves session, exact bytes, hash, head and attribution"
    );
}
