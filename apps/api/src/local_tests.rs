async fn local_request(
    state: &AppState,
    method: &str,
    uri: &str,
    body: Option<String>,
    bearer: Option<&str>,
    host: &str,
    origin: Option<&str>,
) -> (StatusCode, serde_json::Value, axum::http::HeaderMap) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", host)
        .header("content-type", "application/json");
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(origin) = origin {
        builder = builder.header("origin", origin);
    }
    let response = app(state.clone())
        .oneshot(builder.body(body.map(Body::from).unwrap_or_else(Body::empty)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null), headers)
}

fn secret(prefix: &str) -> (String, String) {
    let (raw, _) = crate::auth::new_token();
    let token = format!("{prefix}{raw}");
    let hash: String = crate::auth::token_hash(&token).iter().map(|b| format!("{b:02x}")).collect();
    (token, hash)
}

// Needs an empty database: scripts/api-check-private.sh gives it its own.
#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private, empty PostgreSQL database"]
async fn local_trust_flow_against_live_database() {
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
    let local = "127.0.0.1:8080";
    let (device, device_hash) = secret(crate::local::DEVICE_PREFIX);
    let claim = |hash: &str| format!(r#"{{"token_sha256":"{hash}","label":"Test computer"}}"#);

    crate::local::FORCE_LOCAL_TRUST.store(false, std::sync::atomic::Ordering::SeqCst);
    let (status, _, _) = local_request(&state, "POST", "/agent/local/claim", Some(claim(&device_hash)), None, local, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "off unless AIDRIVE_LOCAL_TRUST=1");
    crate::local::FORCE_LOCAL_TRUST.store(true, std::sync::atomic::Ordering::SeqCst);

    let (status, _, _) = local_request(&state, "POST", "/agent/local/claim", Some(claim(&device_hash)), None, local, Some("https://evil.test")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a web page cannot claim the owner");
    let (status, _, _) = local_request(&state, "POST", "/agent/local/claim", Some(claim(&device_hash)), None, "rebound.evil.test:8080", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "DNS rebinding is refused");

    let (status, body, _) = local_request(&state, "POST", "/agent/local/claim", Some(claim(&device_hash)), None, local, None).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!((body["owner_email"].as_str(), body["created_owner"].as_bool()), (Some("owner@local.invalid"), Some(true)));
    let (status, _, _) = local_request(&state, "POST", "/agent/local/claim", Some(claim(&device_hash)), None, local, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "a device hash is used once");
    let (second_device, second_hash) = secret(crate::local::DEVICE_PREFIX);
    let (status, body, _) = local_request(&state, "POST", "/agent/local/claim", Some(claim(&second_hash)), None, local, None).await;
    assert_eq!((status, body["created_owner"].as_bool()), (StatusCode::CREATED, Some(false)), "same single owner");

    let (agent, agent_hash) = secret("aidrive_agent_");
    let connect = |hash: &str| format!(r#"{{"client_name":"Claude Code","token_sha256":"{hash}","workspace_name":" Side project "}}"#);
    let (status, _, _) = local_request(&state, "POST", "/agent/local/connect", Some(connect(&agent_hash)), Some(&format!("{}{}", crate::local::DEVICE_PREFIX, "0".repeat(64))), local, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "unknown device");
    let (status, body, _) = local_request(&state, "POST", "/agent/local/connect", Some(connect(&agent_hash)), Some(&agent), local, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "an agent key is not a device key: {body}");
    let (status, body, _) = local_request(&state, "POST", "/agent/local/connect", Some(connect(&agent_hash)), Some(&device), local, None).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!((body["workspace_name"].as_str(), body["created_workspace"].as_bool(), body["scope"].as_str()), (Some("Side project"), Some(true), Some("files:write")));
    let workspace = body["workspace_id"].as_str().unwrap().to_owned();

    let (status, body, _) = local_request(&state, "GET", &format!("/agent/workspaces/{workspace}"), None, Some(&agent), local, None).await;
    assert_eq!((status, body["name"].as_str()), (StatusCode::OK, Some("Side project")), "the issued key works: {body}");

    let (codex, codex_hash) = secret("aidrive_agent_");
    let (status, body, _) = local_request(&state, "POST", "/agent/local/connect", Some(connect(&codex_hash)), Some(&second_device), local, None).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!((body["workspace_id"].as_str(), body["created_workspace"].as_bool()), (Some(workspace.as_str()), Some(false)), "the second assistant joins the same space");
    let (status, _, _) = local_request(&state, "GET", &format!("/agent/workspaces/{workspace}"), None, Some(&codex), local, None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, body, _) = local_request(&state, "POST", "/agent/local/login", None, Some(&device), local, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let url = body["url"].as_str().unwrap();
    let path = url.strip_prefix("http://localhost:3000/api").expect(url).to_owned();
    let (status, _, headers) = local_request(&state, "GET", &path, None, None, "localhost:3000", None).await;
    assert_eq!((status, headers["location"].to_str().unwrap()), (StatusCode::SEE_OTHER, "http://localhost:3000/"));
    let cookie = headers["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_owned();
    let (status, body, _) = call(&state, "GET", "/workspaces", Some(&cookie), None).await;
    assert!(status == StatusCode::OK && body.contains("Side project"), "signed in as the owner: {body}");
    let (status, _, headers) = local_request(&state, "GET", &path, None, None, "localhost:3000", None).await;
    assert_eq!((status, headers["location"].to_str().unwrap()), (StatusCode::SEE_OTHER, "http://localhost:3000/?signin=expired"), "single use");
    assert!(!headers.contains_key("set-cookie"));

    // A5: opening the web app on this computer signs the owner in, no form.
    let browser = |headers: &'static [(&'static str, &'static str)]| {
        let state = state.clone();
        async move {
            let mut builder = Request::builder().method("POST").uri("/local/session");
            for (k, v) in headers {
                builder = builder.header(*k, *v);
            }
            let response = app(state).oneshot(builder.body(Body::empty()).unwrap()).await.unwrap();
            let status = response.status();
            let cookie = response.headers().get("set-cookie").and_then(|v| v.to_str().ok()).map(|v| v.split(';').next().unwrap().to_owned());
            let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
            (status, serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_default(), cookie)
        }
    };
    let page: &'static [(&'static str, &'static str)] = &[("host", "127.0.0.1:8080"), ("x-forwarded-host", "localhost:3000"), ("sec-fetch-site", "same-origin")];
    let (status, body, cookie) = browser(page).await;
    assert_eq!((status, body["local"].as_bool()), (StatusCode::OK, Some(true)), "{body}");
    let cookie = cookie.expect("session cookie");
    let (status, body, _) = call(&state, "GET", "/auth/session", Some(&cookie), None).await;
    assert!(status == StatusCode::OK && body.contains("owner@local.invalid"), "the owner is signed in: {body}");
    let (status, body, _) = browser(&[("host", "127.0.0.1:8080"), ("x-forwarded-host", "evil.test:3000"), ("sec-fetch-site", "same-origin")]).await;
    assert_eq!((status, body["error"].as_str()), (StatusCode::FORBIDDEN, Some("not_local_browser")), "DNS-rebound page");
    let (status, _, cookie) = browser(&[("host", "localhost:3000"), ("sec-fetch-site", "cross-site"), ("origin", "https://evil.test")]).await;
    assert!(status == StatusCode::FORBIDDEN && cookie.is_none(), "cross-site request");
    let (status, body, _) = call(&state, "GET", "/local/mode", None, None).await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, r#"{"local":true}"#));
    println!("PASS local session: the web app on this computer opens signed in as the owner; DNS-rebound and cross-site requests refused");

    let creds = r#"{"email":"second@live.test","password":"correct horse battery staple"}"#;
    let (status, _, _) = call(&state, "POST", "/auth/register", None, Some(creds)).await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, third_hash) = secret(crate::local::DEVICE_PREFIX);
    let (status, body, _) = local_request(&state, "POST", "/agent/local/claim", Some(claim(&third_hash)), None, local, None).await;
    assert_eq!((status, body["error"].as_str()), (StatusCode::CONFLICT, Some("not_single_person_install")), "{body}");
    let (status, body, _) = browser(page).await;
    assert_eq!((status, body["error"].as_str()), (StatusCode::CONFLICT, Some("not_single_person_install")), "two accounts: no automatic sign-in");

    crate::local::FORCE_LOCAL_TRUST.store(false, std::sync::atomic::Ordering::SeqCst);
    let (status, _, _) = local_request(&state, "POST", "/agent/local/connect", Some(connect(&agent_hash)), Some(&device), local, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = browser(page).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "hosted or non-loopback install: no automatic sign-in");
    let (_, body, _) = call(&state, "GET", "/local/mode", None, None).await;
    assert_eq!(body, r#"{"local":false}"#, "the web app keeps the password form");
    println!("PASS local trust: claim, connect, reuse, sign-in link, refusals");
}
