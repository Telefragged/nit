//! Static UI serving (docs/api.md "Static UI"): the built SPA outside
//! /api, index.html fallback for client-side routes, API-only without a
//! web dist. Client routes are change-id addressed now (`/chains/{change_id}`,
//! `/changes/{id}`).

mod common;

use common::*;

#[test]
fn serves_spa_with_index_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let dist = dir.path().join("dist");
    std::fs::create_dir_all(dist.join("assets")).unwrap();
    std::fs::write(dist.join("index.html"), "<html>nit-spa</html>").unwrap();
    std::fs::write(dist.join("assets/app.js"), "console.log('nit')").unwrap();

    let server = TestServer::start(dir.path().join("nit.sqlite3"), Some(dist));

    // Real files serve as-is.
    let (st, body) = http_get(&server.url("/index.html"));
    assert_eq!(st, 200);
    assert_eq!(body.as_str().unwrap(), "<html>nit-spa</html>");
    let (st, body) = http_get(&server.url("/assets/app.js"));
    assert_eq!(st, 200);
    assert_eq!(body.as_str().unwrap(), "console.log('nit')");

    // Client-side routes fall back to index.html — now change-id addressed.
    for route in ["/", "/chains/12", "/changes/10"] {
        let (st, body) = http_get(&server.url(route));
        assert_eq!(st, 200, "{route}");
        assert_eq!(body.as_str().unwrap(), "<html>nit-spa</html>", "{route}");
    }

    // /api routes win over the SPA.
    let (st, health) = http_get(&server.url("/api/health"));
    assert_eq!(st, 200);
    assert_eq!(health["status"], "ok");
    assert_eq!(health["version"], nit::VERSION);
    // An unknown change is a JSON 404, never the SPA.
    let (st, e) = http_get(&server.url("/api/chains/12"));
    assert_eq!(st, 404);
    assert!(e["error"].is_string());
}

#[test]
fn runs_api_only_without_web_dist() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path().join("nit.sqlite3"), None);

    let (st, health) = http_get(&server.url("/api/health"));
    assert_eq!(st, 200);
    assert_eq!(health["status"], "ok");

    // No SPA → client routes are a bare 404 (no index.html to fall back to).
    let (st, _) = http_get(&server.url("/chains/12"));
    assert_eq!(st, 404);
}

/// api.md line 1: everything under /api is JSON in/out — including paths
/// axum rejects before a handler runs (unknown endpoint, bad path/body
/// types, wrong method). None of it may fall through to the SPA.
#[test]
fn api_errors_are_json_everywhere() {
    let dir = tempfile::tempdir().unwrap();
    let dist = dir.path().join("dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("index.html"), "<html>nit-spa</html>").unwrap();
    let server = TestServer::start(dir.path().join("nit.sqlite3"), Some(dist));

    // Unknown /api paths: JSON 404, not the SPA.
    for path in ["/api", "/api/", "/api/nonexistent", "/api/chain/12"] {
        let (st, body) = http_get(&server.url(path));
        assert_eq!(st, 404, "{path}: {body}");
        assert!(body["error"].is_string(), "{path}: {body}");
    }

    // Non-numeric path param: JSON 400.
    let (st, body) = http_get(&server.url("/api/chains/abc"));
    assert_eq!(st, 400, "{body}");
    assert!(body["error"].is_string(), "{body}");

    // Malformed JSON body: JSON 400, not text/plain.
    let resp = ureq::Agent::new_with_defaults()
        .post(&server.url("/api/push"))
        .header("content-type", "application/json")
        .config()
        .http_status_as_error(false)
        .build()
        .send("{not json")
        .unwrap();
    let st = resp.status().as_u16();
    let body: serde_json::Value =
        serde_json::from_str(&resp.into_body().read_to_string().unwrap()).unwrap();
    assert_eq!(st, 400, "{body}");
    assert!(body["error"].is_string(), "{body}");

    // Wrong method: JSON 405.
    let resp = ureq::Agent::new_with_defaults()
        .delete(&server.url("/api/health"))
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .unwrap();
    let st = resp.status().as_u16();
    let body: serde_json::Value =
        serde_json::from_str(&resp.into_body().read_to_string().unwrap()).unwrap();
    assert_eq!(st, 405, "{body}");
    assert!(body["error"].is_string(), "{body}");
}
