// The owner's web app as static files (`next build` with AIDRIVE_WEB_EXPORT=1),
// served by the API itself so an installed copy needs no Node.js. The API is
// also answered under /api, which is where the web app calls it.
use axum::{
    Router,
    body::Body,
    http::{HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use std::path::{Component, Path, PathBuf};

pub fn dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("AIDRIVE_WEB_DIR")?);
    dir.join("index.html").is_file().then_some(dir)
}

pub fn with_front(api: Router, dir: PathBuf) -> Router {
    Router::new()
        .nest("/api", api.clone())
        .merge(api)
        .fallback(move |uri: Uri| {
            let dir = dir.clone();
            async move { serve(&dir, uri.path()).await }
        })
}

fn resolve(dir: &Path, path: &str) -> Option<PathBuf> {
    let mut relative = PathBuf::new();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        if part.contains(['\\', ':', '%']) {
            return None;
        }
        match Path::new(part).components().next() {
            Some(Component::Normal(_)) => relative.push(part),
            _ => return None,
        }
    }
    let base = dir.join(&relative);
    if relative.as_os_str().is_empty() {
        return Some(dir.join("index.html"));
    }
    if base.is_file() {
        return Some(base);
    }
    let html = base.with_extension("html");
    if html.is_file() {
        return Some(html);
    }
    let index = base.join("index.html");
    index.is_file().then_some(index)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "txt" => "text/x-component",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

async fn serve(dir: &Path, path: &str) -> Response {
    let (status, file) = match resolve(dir, path) {
        Some(file) => (StatusCode::OK, file),
        None => (StatusCode::NOT_FOUND, dir.join("404.html")),
    };
    let Ok(bytes) = tokio::fs::read(&file).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut response = (status, Body::from(bytes)).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(&file)),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    // The review inbox is where the owner approves; no other site may frame it.
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("frame-ancestors 'none'"),
    );
    let cache = if path.starts_with("/_next/static/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    response
}

#[cfg(test)]
mod tests {
    use super::resolve;
    use std::fs;

    #[test]
    fn paths_stay_inside_the_web_folder() {
        let dir = std::env::temp_dir().join(format!("aidrive-web-{}", std::process::id()));
        fs::create_dir_all(dir.join("_next")).unwrap();
        fs::write(dir.join("index.html"), "i").unwrap();
        fs::write(dir.join("pair.html"), "p").unwrap();
        fs::write(dir.join("_next").join("a.js"), "a").unwrap();
        assert_eq!(resolve(&dir, "/"), Some(dir.join("index.html")));
        assert_eq!(resolve(&dir, "/pair"), Some(dir.join("pair.html")));
        assert_eq!(
            resolve(&dir, "/_next/a.js"),
            Some(dir.join("_next").join("a.js"))
        );
        for bad in [
            "/../Cargo.toml",
            "/_next/../../x",
            "/%2e%2e/x",
            "/C:/Windows",
            "/a\\..\\b",
            "/missing",
        ] {
            assert_eq!(resolve(&dir, bad), None, "{bad}");
        }
        fs::remove_dir_all(&dir).unwrap();
    }
}
