async fn agent_call(
    state: &AppState,
    method: &str,
    uri: &str,
    token: &str,
) -> (StatusCode, String) {
    let response = app(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    if uri.starts_with("/agent/") {
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    }
    let body = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private database and fixture administrator"]
async fn agents_flow_against_live_database() {
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
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
    for email in ["grant-owner@live.test", "grant-other@live.test"] {
        let creds = format!(r#"{{"email":"{email}","password":"correct horse battery staple"}}"#);
        let (status, body, cookie) =
            call(&state, "POST", "/auth/register", None, Some(&creds)).await;
        assert_eq!(status, StatusCode::CREATED);
        users.push(json_field(&body, "id"));
        cookies.push(cookie.unwrap());
    }
    let mut spaces = Vec::new();
    for cookie in &cookies {
        let (status, body, _) = call(
            &state,
            "POST",
            "/workspaces",
            Some(cookie),
            Some(r#"{"name":"Agent test"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        spaces.push(json_field(&body, "id"));
    }
    let endpoint = format!("/workspaces/{}/agents", spaces[0]);
    let read = format!("/agent/workspaces/{}", spaces[0]);
    let payload = |n| {
        format!(r#"{{"request_id":"10000000-0000-4000-8000-{n:012}","name":"Local assistant"}}"#)
    };
    let (status, body, _) = call(
        &state,
        "POST",
        &endpoint,
        Some(&cookies[0]),
        Some(&payload(1)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let token = json_field(&body, "token");
    let grant = json_field(&body, "id");
    assert_eq!(token.len(), "aidrive_agent_".len() + 64);
    assert_eq!(
        call(
            &state,
            "POST",
            &endpoint,
            Some(&cookies[0]),
            Some(&payload(1))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (status, listing, _) = call(&state, "GET", &endpoint, Some(&cookies[0]), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        listing.contains(&grant) && !listing.contains(&token) && !listing.contains("token_hash")
    );
    assert_eq!(listing.matches("Local assistant").count(), 1);
    let stored: Vec<u8> =
        sqlx::query_scalar("SELECT token_hash FROM agent_grants WHERE id = $1::uuid")
            .bind(&grant)
            .fetch_one(&admin)
            .await
            .unwrap();
    assert_eq!(stored, crate::auth::token_hash(&token));
    assert!(
        sqlx::query("SELECT token_hash FROM agent_grants")
            .execute(&db)
            .await
            .is_err()
    );
    let (status, body) = agent_call(&state, "GET", &read, &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("workspace:read") && !body.contains("owner"));
    assert_eq!(
        agent_call(
            &state,
            "GET",
            &format!("/agent/workspaces/{}", spaces[1]),
            &token
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        agent_call(&state, "GET", "/workspaces", &token).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        agent_call(&state, "POST", &endpoint, &token).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        agent_call(&state, "GET", &read, "bad").await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&state, "GET", &read, Some(&cookies[0]), None).await.0,
        StatusCode::UNAUTHORIZED
    );
    for role in [None, Some("viewer"), Some("editor")] {
        if let Some(role) = role {
            sqlx::query("INSERT INTO memberships (workspace_id, user_id, role) VALUES ($1::uuid,$2::uuid,$3) ON CONFLICT (workspace_id,user_id) DO UPDATE SET role=excluded.role")
                .bind(&spaces[0]).bind(&users[1]).bind(role).execute(&admin).await.unwrap();
        }
        for method in ["GET", "POST", "DELETE"] {
            let uri = if method == "DELETE" {
                format!("{endpoint}/{grant}")
            } else {
                endpoint.clone()
            };
            assert_eq!(
                call(&state, method, &uri, Some(&cookies[1]), Some(&payload(2)))
                    .await
                    .0,
                StatusCode::NOT_FOUND,
                "{method} {role:?}"
            );
        }
    }
    let extra = payload(2).replace(
        "Local assistant\"",
        "Local assistant\",\"scope\":\"files:admin\"",
    );
    assert_eq!(
        call(&state, "POST", &endpoint, Some(&cookies[0]), Some(&extra))
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    for _ in 0..2 {
        assert_eq!(
            call(
                &state,
                "DELETE",
                &format!("{endpoint}/{grant}"),
                Some(&cookies[0]),
                None
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
    }
    assert_eq!(
        agent_call(&state, "GET", &read, &token).await.0,
        StatusCode::UNAUTHORIZED
    );
    println!(
        "PASS agent isolation, least privilege, hash-only storage, one-time issuance, retry conflict and revocation"
    );

    let (_, body, _) = call(
        &state,
        "POST",
        &endpoint,
        Some(&cookies[0]),
        Some(&payload(3)),
    )
    .await;
    let expired = json_field(&body, "token");
    sqlx::query("UPDATE agent_grants SET expires_at = clock_timestamp() - interval '1 second' WHERE id = $1::uuid")
        .bind(json_field(&body, "id")).execute(&admin).await.unwrap();
    assert_eq!(
        agent_call(&state, "GET", &read, &expired).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, body, _) = call(
        &state,
        "POST",
        &endpoint,
        Some(&cookies[0]),
        Some(&payload(4)),
    )
    .await;
    let demoted = json_field(&body, "token");
    sqlx::query(
        "UPDATE memberships SET role='owner' WHERE workspace_id=$1::uuid AND user_id=$2::uuid",
    )
    .bind(&spaces[0])
    .bind(&users[1])
    .execute(&admin)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE memberships SET role='editor' WHERE workspace_id=$1::uuid AND user_id=$2::uuid",
    )
    .bind(&spaces[0])
    .bind(&users[0])
    .execute(&admin)
    .await
    .unwrap();
    assert_eq!(
        agent_call(&state, "GET", &read, &demoted).await.0,
        StatusCode::UNAUTHORIZED
    );
    sqlx::query(
        "UPDATE memberships SET role='owner' WHERE workspace_id=$1::uuid AND user_id=$2::uuid",
    )
    .bind(&spaces[0])
    .bind(&users[0])
    .execute(&admin)
    .await
    .unwrap();
    assert_eq!(
        agent_call(&state, "GET", &read, &demoted).await.0,
        StatusCode::UNAUTHORIZED
    );
    println!(
        "PASS agent expiry and permanent revocation on issuer demotion, including re-promotion"
    );

    let (_, body, _) = call(
        &state,
        "POST",
        &endpoint,
        Some(&cookies[0]),
        Some(&payload(5)),
    )
    .await;
    let racing_token = json_field(&body, "token");
    let racing_id = json_field(&body, "id");
    let mut revocation = admin.begin().await.unwrap();
    sqlx::query("UPDATE agent_grants SET revoked_at=clock_timestamp() WHERE id=$1::uuid")
        .bind(&racing_id)
        .execute(&mut *revocation)
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&db)
        .await
        .unwrap();
    let racing_state = state.clone();
    let racing_read = read.clone();
    let reader =
        tokio::spawn(
            async move { agent_call(&racing_state, "GET", &racing_read, &racing_token).await },
        );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid=$1 AND wait_event_type='Lock')")
                .bind(pid).fetch_one(&admin).await.unwrap();
            if waiting { break; }
            assert!(!reader.is_finished(), "reader did not wait for pending revocation");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("reader should be blocked on the grant row");
    revocation.commit().await.unwrap();
    assert_eq!(reader.await.unwrap().0, StatusCode::UNAUTHORIZED);
    let context: (Option<String>, Option<String>) = sqlx::query_as("SELECT nullif(current_setting('app.user_id', true), ''), nullif(current_setting('app.workspace_id', true), '')")
        .fetch_one(&db).await.unwrap();
    assert_eq!(context, (None, None));
    println!(
        "PASS concurrent read waits for revocation and rechecks permission; pooled identity cleared"
    );
    let (_, body, _) = call(
        &state,
        "POST",
        &endpoint,
        Some(&cookies[0]),
        Some(&payload(6)),
    )
    .await;
    let expiry_token = json_field(&body, "token");
    let expiry_id = json_field(&body, "id");
    sqlx::query("UPDATE agent_grants SET expires_at=clock_timestamp() + interval '2 seconds' WHERE id=$1::uuid")
        .bind(&expiry_id).execute(&admin).await.unwrap();
    let mut held = admin.begin().await.unwrap();
    sqlx::query("SELECT id FROM agent_grants WHERE id=$1::uuid FOR UPDATE")
        .bind(&expiry_id)
        .execute(&mut *held)
        .await
        .unwrap();
    let expiry_state = state.clone();
    let expiry_read = read.clone();
    let reader =
        tokio::spawn(
            async move { agent_call(&expiry_state, "GET", &expiry_read, &expiry_token).await },
        );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid=$1 AND wait_event_type='Lock')")
                .bind(pid).fetch_one(&admin).await.unwrap();
            if waiting { break; }
            assert!(!reader.is_finished(), "reader did not wait for grant lock");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        loop {
            let expired: bool = sqlx::query_scalar("SELECT expires_at <= clock_timestamp() FROM agent_grants WHERE id=$1::uuid")
                .bind(&expiry_id).fetch_one(&admin).await.unwrap();
            if expired { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("grant should expire while reader waits");
    held.commit().await.unwrap();
    assert_eq!(reader.await.unwrap().0, StatusCode::UNAUTHORIZED);
    println!("PASS expiry is rechecked after a lock wait, even when the locked row was unchanged");
    let (_, body, _) = call(
        &state,
        "POST",
        &endpoint,
        Some(&cookies[0]),
        Some(&payload(7)),
    )
    .await;
    let departed_token = json_field(&body, "token");
    sqlx::query("DELETE FROM memberships WHERE workspace_id=$1::uuid AND user_id=$2::uuid")
        .bind(&spaces[0])
        .bind(&users[0])
        .execute(&admin)
        .await
        .unwrap();
    assert_eq!(
        agent_call(&state, "GET", &read, &departed_token).await.0,
        StatusCode::UNAUTHORIZED
    );
    sqlx::query(
        "INSERT INTO memberships (workspace_id,user_id,role) VALUES ($1::uuid,$2::uuid,'owner')",
    )
    .bind(&spaces[0])
    .bind(&users[0])
    .execute(&admin)
    .await
    .unwrap();
    assert_eq!(
        agent_call(&state, "GET", &read, &departed_token).await.0,
        StatusCode::UNAUTHORIZED
    );
    println!("PASS owner removal and rejoining cannot reactivate a key");
    db.close().await;
    admin.close().await;
}
