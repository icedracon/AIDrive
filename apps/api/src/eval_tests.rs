// Memory evaluation, development set (docs/PRODUCT.md, "Memory evaluation
// protocol"). Seeds eval/dev/tutorbook.json through the real API, runs every
// case as the owner's assistant, and prints recall@10 by language and
// category, an exact random-k control, abstention and forbidden-source
// counts. Only invariants are asserted: on the development set recall is a
// measurement to tune against, not a release gate.
#[tokio::test]
#[ignore = "scripts/api-check.sh supplies a private PostgreSQL instance"]
async fn memory_eval_dev_set() {
    use std::collections::{BTreeMap, HashMap, HashSet};

    fn enc(text: &str) -> String {
        text.bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
            .collect()
    }
    fn rid(n: &mut u32) -> String {
        *n += 1;
        format!("30000000-0000-4000-8000-{:012}", *n)
    }

    let db = sqlx::postgres::PgPoolOptions::new().max_connections(1)
        .connect(&std::env::var("AIDRIVE_LIVE_TEST_URL").unwrap()).await.unwrap();
    let admin = sqlx::PgPool::connect(&std::env::var("AIDRIVE_FIXTURE_ADMIN_URL").unwrap()).await.unwrap();
    let state = AppState { db: Some(db.clone()), web_origin: "http://localhost:3000".into(), allow_signup: true };
    // AIDRIVE_EVAL_SET=<path> runs another set (the locked held-out set) instead of eval/dev.
    let fixture: serde_json::Value = match std::env::var("AIDRIVE_EVAL_SET") {
        Ok(path) => serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap(),
        Err(_) => serde_json::from_str(include_str!("../../../eval/dev/tutorbook.json")).unwrap(),
    };
    let mut n = 0u32;

    let (mut users, mut cookies) = (Vec::new(), Vec::new());
    for email in ["eval-owner@live.test", "eval-editor@live.test"] {
        let creds = format!(r#"{{"email":"{email}","password":"correct horse battery staple"}}"#);
        let (_, body, cookie) = call(&state, "POST", "/auth/register", None, Some(&creds)).await;
        users.push(json_field(&body, "id"));
        cookies.push(cookie.unwrap());
    }
    let (owner, editor) = (cookies[0].clone(), cookies[1].clone());
    let mut spaces = Vec::new();
    for name in ["Eval", "Eval other"] {
        let (_, body, _) = call(&state, "POST", "/workspaces", Some(&owner), Some(&format!(r#"{{"name":"{name}","review":"strict"}}"#))).await;
        spaces.push(json_field(&body, "id"));
    }
    let (ws, ws2) = (spaces[0].clone(), spaces[1].clone());
    sqlx::query("INSERT INTO memberships (workspace_id, user_id, role) VALUES ($1::uuid, $2::uuid, 'editor')")
        .bind(&ws).bind(&users[1]).execute(&admin).await.unwrap();
    let mut tokens = Vec::new();
    for space in [&ws, &ws2] {
        let payload = format!(r#"{{"request_id":"{}","name":"eval","scope":"files:write"}}"#, rid(&mut n));
        let (_, body, _) = call(&state, "POST", &format!("/workspaces/{space}/agents"), Some(&owner), Some(&payload)).await;
        tokens.push(serde_json::from_str::<serde_json::Value>(&body).unwrap()["token"].as_str().unwrap().to_owned());
    }
    let (token, token2) = (tokens[0].clone(), tokens[1].clone());
    let agent = format!("/agent/workspaces/{ws}");

    // Seed the project history step by step, keeping symbolic keys.
    let mut files: HashMap<String, (String, String)> = HashMap::new();
    let mut sessions: HashMap<String, String> = HashMap::new();
    let mut event_keys: HashMap<(String, i64), String> = HashMap::new();
    let mut items: HashMap<String, String> = HashMap::new();
    let mut item_keys: HashMap<String, String> = HashMap::new();
    let mut valid_from: HashMap<String, String> = HashMap::new();
    for step in fixture["steps"].as_array().unwrap() {
        let key = step["key"].as_str().unwrap().to_owned();
        match step["op"].as_str().unwrap() {
            "file" => {
                let content = step["content"].as_str().unwrap();
                let (status, v) = match files.get(&key) {
                    Some((id, base)) => agent_send(&state, "PUT", &format!("{agent}/files/{id}"), &token, Some(serde_json::json!(
                        {"request_id": rid(&mut n), "base_version_id": base, "content": content}).to_string())).await,
                    None => agent_send(&state, "POST", &format!("{agent}/files"), &token, Some(serde_json::json!(
                        {"request_id": rid(&mut n), "name": step["name"], "content": content}).to_string())).await,
                };
                assert!(status.is_success(), "file {key}: {v}");
                files.insert(key, (v["file_id"].as_str().unwrap().to_owned(), v["version_id"].as_str().unwrap().to_owned()));
            }
            "session" => {
                let meta = serde_json::json!({"client": step["client"], "external_id": key, "label": step["label"], "origin": "import"}).to_string();
                let start = step["start"].as_str().unwrap();
                let events: Vec<serde_json::Value> = step["events"].as_array().unwrap().iter().enumerate()
                    .map(|(i, e)| serde_json::json!({"key": format!("e{i}"), "ordinal": i, "role": e[0], "at": format!("{start}:{i:02}:00Z"), "text": e[1]}))
                    .collect();
                let batch = serde_json::json!({"complete": true, "events": events}).to_string();
                let sid = match step["actor"].as_str().unwrap() {
                    "editor" => {
                        let base = format!("/workspaces/{ws}/sessions");
                        let (_, body, _) = call(&state, "POST", &base, Some(&editor), Some(&meta)).await;
                        let sid = json_field(&body, "session_id");
                        call(&state, "POST", &format!("{base}/{sid}/events"), Some(&editor), Some(&batch)).await;
                        sid
                    }
                    actor => {
                        let (base, tok) = if actor == "other" { (format!("/agent/workspaces/{ws2}/sessions"), &token2) } else { (format!("{agent}/sessions"), &token) };
                        let (_, v) = agent_send(&state, "POST", &base, tok, Some(meta)).await;
                        let sid = v["session_id"].as_str().unwrap().to_owned();
                        agent_send(&state, "POST", &format!("{base}/{sid}/events"), tok, Some(batch)).await;
                        sid
                    }
                };
                for i in 0..step["events"].as_array().unwrap().len() {
                    event_keys.insert((sid.clone(), i as i64), format!("{key}#{i}"));
                }
                sessions.insert(key, sid);
            }
            "delete_session" => {
                let (status, _, _) = call(&state, "DELETE", &format!("/workspaces/{ws}/sessions/{}", sessions[&key]), Some(&owner), None).await;
                assert_eq!(status, StatusCode::NO_CONTENT);
            }
            "propose" => {
                let evidence: Vec<serde_json::Value> = step["evidence"].as_array().unwrap().iter().map(|r| {
                    let r = r.as_str().unwrap();
                    match r.strip_prefix("file:") {
                        Some(f) => serde_json::json!({"file_id": files[f].0, "version_id": files[f].1}),
                        None => {
                            let (s, o) = r.split_once('#').unwrap();
                            serde_json::json!({"session_id": sessions[s], "ordinal": o.parse::<i64>().unwrap()})
                        }
                    }
                }).collect();
                let mut body = serde_json::json!({"request_id": rid(&mut n), "kind": step["kind"], "topic": step["topic"],
                    "statement": step["statement"], "evidence": evidence});
                if let Some(o) = step["outcome"].as_str() { body["outcome"] = o.into(); }
                if let Some(s) = step["supersedes"].as_str() { body["supersedes"] = items[s].clone().into(); }
                let (status, item) = agent_send(&state, "POST", &format!("{agent}/knowledge"), &token, Some(body.to_string())).await;
                assert_eq!(status, StatusCode::CREATED, "propose {key}: {item}");
                let id = item["id"].as_str().unwrap().to_owned();
                if step["decide"] == "approved" {
                    let (status, decided, _) = call(&state, "POST", &format!("/workspaces/{ws}/knowledge/{id}/decision"), Some(&owner),
                        Some(r#"{"decision":"approved"}"#)).await;
                    assert_eq!(status, StatusCode::OK, "approve {key}: {decided}");
                    let decided: serde_json::Value = serde_json::from_str(&decided).unwrap();
                    valid_from.insert(key.clone(), decided["valid_from"].as_str().unwrap().to_owned());
                }
                item_keys.insert(id.clone(), key.clone());
                items.insert(key, id);
            }
            other => panic!("unknown step {other}"),
        }
    }

    // Pools for the random-k control: everything the assistant could have been shown.
    let (_, listed) = agent_send(&state, "GET", &format!("{agent}/sessions?limit=100"), &token, None).await;
    let session_pool: i64 = listed.as_array().unwrap().iter().map(|s| s["event_count"].as_i64().unwrap()).sum();
    let (_, current) = agent_send(&state, "GET", &format!("{agent}/knowledge?limit=200"), &token, None).await;
    let memory_pool = current["items"].as_array().unwrap().len() as f64;

    // Never visible to the owner's assistant: another person's private session,
    // another workspace, deleted sessions, proposals never approved.
    let steps = fixture["steps"].as_array().unwrap();
    let deleted: HashSet<&str> = steps.iter().filter(|s| s["op"] == "delete_session").filter_map(|s| s["key"].as_str()).collect();
    let mut never_keys: Vec<String> = Vec::new();
    for s in steps {
        let key = s["key"].as_str().unwrap();
        match s["op"].as_str().unwrap() {
            "session" if s["actor"] != "agent" || deleted.contains(key) => {
                never_keys.extend((0..s["events"].as_array().unwrap().len()).map(|i| format!("{key}#{i}")));
            }
            "propose" if s["decide"] != "approved" => never_keys.push(key.to_owned()),
            _ => {}
        }
    }
    let never: HashSet<&str> = never_keys.iter().map(String::as_str).collect();
    let superseded: HashSet<&str> = steps.iter().filter_map(|s| s["supersedes"].as_str()).collect();
    let (mut leaks, mut stale_current, mut temporal_wrong) = (0usize, 0usize, 0usize);
    let mut by_lang: BTreeMap<String, (f64, f64, usize)> = BTreeMap::new();
    let mut by_cat: BTreeMap<String, (f64, f64, usize)> = BTreeMap::new();
    let (mut total, mut random_total, mut answerable) = (0.0, 0.0, 0usize);
    let (mut abst_ok, mut abst_n, mut abst_hits) = (0usize, 0usize, 0usize);
    let (mut flags_ok, mut flags_n) = (0usize, 0usize);
    let mut misses = Vec::new();
    // AIDRIVE_EVAL_DUMP=<path>: write each case's ranked sources, flags and
    // latency plus the visible pools, for offline retrieval experiments (T3).
    let mut dump_cases = Vec::new();
    let mut dump_pools: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    for case in fixture["cases"].as_array().unwrap() {
        let (id, lang, cat, tool) = (case["id"].as_str().unwrap(), case["lang"].as_str().unwrap(),
            case["category"].as_str().unwrap(), case["tool"].as_str().unwrap());
        let q = enc(case["query"].as_str().unwrap());
        let mut got: Vec<String> = Vec::new();
        let mut flagged: HashMap<String, (bool, bool)> = HashMap::new();
        let mut pool = 0.0;
        let (mut sess_got, mut mem_got, mut sess_ms, mut mem_ms) = (Vec::new(), Vec::new(), 0.0, 0.0);
        if matches!(tool, "sessions" | "both") {
            let t0 = std::time::Instant::now();
            let (status, hits) = agent_send(&state, "GET", &format!("{agent}/sessions/search?q={q}&limit=10"), &token, None).await;
            assert_eq!(status, StatusCode::OK, "{id}: {hits}");
            for h in hits.as_array().unwrap() {
                let k = (h["session_id"].as_str().unwrap().to_owned(), h["ordinal"].as_i64().unwrap());
                got.push(event_keys.get(&k).cloned().unwrap_or_else(|| format!("?{}#{}", k.0, k.1)));
            }
            sess_ms = t0.elapsed().as_secs_f64() * 1000.0;
            sess_got = got.clone();
            pool = session_pool as f64;
        }
        if matches!(tool, "memory" | "both") {
            let as_of = case["as_of"].as_str().map(|k| format!("&as_of={}", enc(&valid_from[k]))).unwrap_or_default();
            let t0 = std::time::Instant::now();
            let (status, list) = agent_send(&state, "GET", &format!("{agent}/knowledge?q={q}&limit=10{as_of}"), &token, None).await;
            assert_eq!(status, StatusCode::OK, "{id}: {list}");
            for i in list["items"].as_array().unwrap() {
                let k = item_keys.get(i["id"].as_str().unwrap()).cloned().unwrap_or_else(|| format!("?{}", i["id"]));
                flagged.insert(k.clone(), (i["stale"] == true, !i["disputed_by"].as_array().unwrap().is_empty()));
                mem_got.push(k.clone());
                got.push(k);
            }
            mem_ms = t0.elapsed().as_secs_f64() * 1000.0;
            let pool_key = case["as_of"].as_str().unwrap_or("current").to_owned();
            let (_, then) = agent_send(&state, "GET", &format!("{agent}/knowledge?limit=200{as_of}"), &token, None).await;
            let then = then["items"].as_array().unwrap().clone();
            pool = if as_of.is_empty() { memory_pool } else { then.len() as f64 };
            dump_pools.entry(pool_key).or_insert_with(|| serde_json::Value::Array(then.iter().map(|i| serde_json::json!({
                "key": item_keys.get(i["id"].as_str().unwrap()).cloned().unwrap_or_default(),
                "stale": i["stale"] == true, "disputed": !i["disputed_by"].as_array().unwrap().is_empty()})).collect()));
            if as_of.is_empty() {
                stale_current += got.iter().filter(|k| superseded.contains(k.as_str())).count();
            }
        }
        dump_cases.push(serde_json::json!({"id": id, "sessions": sess_got, "memory": mem_got,
            "sessions_ms": sess_ms, "memory_ms": mem_ms}));
        got.truncate(if tool == "both" { 20 } else { 10 });
        let forbid: Vec<&str> = case["forbid"].as_array().map(|f| f.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
        if case["as_of"].is_null() {
            // Superseded items are counted separately as stale_current.
            leaks += got.iter().filter(|k| never.contains(k.as_str()) || (forbid.contains(&k.as_str()) && !superseded.contains(k.as_str()))).count();
        } else {
            leaks += got.iter().filter(|k| never.contains(k.as_str())).count();
            temporal_wrong += got.iter().filter(|k| forbid.contains(&k.as_str())).count();
        }
        let expect: Vec<&str> = case["expect"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
        if cat == "abstention" {
            abst_n += 1;
            abst_hits += got.len();
            if got.is_empty() { abst_ok += 1; } else { misses.push(format!("{id}(abstain:{} hits)", got.len())); }
            continue;
        }
        if expect.is_empty() {
            continue;
        }
        let found = expect.iter().filter(|e| got.iter().any(|g| g == *e)).count();
        let recall = found as f64 / expect.len() as f64;
        let random = (10.0 / pool.max(1.0)).min(1.0);
        if let Some(flag) = case["flag"].as_str() {
            flags_n += 1;
            let ok = expect.iter().all(|e| flagged.get(*e).is_some_and(|(stale, disputed)| if flag == "stale" { *stale } else { *disputed }));
            if ok { flags_ok += 1; }
        }
        answerable += 1;
        total += recall;
        random_total += random;
        for (map, k) in [(&mut by_lang, lang), (&mut by_cat, cat)] {
            let e = map.entry(k.to_owned()).or_insert((0.0, 0.0, 0));
            e.0 += recall;
            e.1 += random;
            e.2 += 1;
        }
        if recall < 1.0 {
            misses.push(format!("{id}({found}/{} got {})", expect.len(), got.iter().take(3).cloned().collect::<Vec<_>>().join(",")));
        }
    }

    if let Ok(path) = std::env::var("AIDRIVE_EVAL_DUMP") {
        let visible: Vec<String> = listed.as_array().unwrap().iter()
            .filter_map(|s| sessions.iter().find(|(_, id)| s["id"].as_str() == Some(id.as_str())).map(|(k, _)| k.clone()))
            .collect();
        let dump = serde_json::json!({"visible_sessions": visible, "knowledge_pools": dump_pools, "cases": dump_cases});
        std::fs::write(&path, serde_json::to_string_pretty(&dump).unwrap()).unwrap();
        println!("EVAL dump written to {path}");
    }
    println!("EVAL dev set: {} cases, {answerable} answerable, pools: {session_pool} session events, {memory_pool} current knowledge items",
        fixture["cases"].as_array().unwrap().len());
    println!("EVAL recall@10 {:.2} (random-k control {:.2})", total / answerable as f64, random_total / answerable as f64);
    for (name, map) in [("language", &by_lang), ("category", &by_cat)] {
        let parts: Vec<String> = map.iter().map(|(k, (r, c, m))| format!("{k} {:.2} vs random {:.2} (n={m})", r / *m as f64, c / *m as f64)).collect();
        println!("EVAL by {name}: {}", parts.join("; "));
    }
    println!("EVAL flags correct {flags_ok}/{flags_n}; abstention with zero hits {abst_ok}/{abst_n} (hits returned: {abst_hits})");
    println!("EVAL forbidden sources returned {leaks}; superseded items returned as current {stale_current}; later items returned for an earlier as_of {temporal_wrong}");
    println!("EVAL misses: {}", misses.join(" "));
    assert_eq!(leaks, 0, "a forbidden source (private, other workspace, deleted or unapproved) was returned");
    assert_eq!(stale_current, 0, "a superseded item was returned as current knowledge");
    assert_eq!(temporal_wrong, 0, "as_of returned an item approved after that moment");
    println!("PASS memory eval dev set: every case ran; no forbidden, deleted, other-workspace, unapproved or superseded source returned; metrics in the EVAL lines");
}
