//! Static UI serving: the built SPA outside /api, index.html fallback
//! for client-side routes, API-only without a web UI. Client routes
//! are id addressed (`/repos/{id}`, `/changes/{id}`).

mod common;

use common::*;
use include_dir::{Dir, DirEntry, File};

static WEB_UI: Dir = Dir::new(
    "",
    &[
        DirEntry::File(File::new("index.html", b"<html>nit-spa</html>")),
        DirEntry::Dir(Dir::new(
            "assets",
            &[DirEntry::File(File::new(
                "assets/app.js",
                b"console.log('nit')",
            ))],
        )),
    ],
);

#[test]
fn serves_spa_with_index_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path().join("nit.sqlite3"), Some(&WEB_UI));

    let (st, body) = http_get(&server.url("/index.html"));
    assert_eq!(st, 200);
    assert_eq!(body.as_str().unwrap(), "<html>nit-spa</html>");
    let (st, body) = http_get(&server.url("/assets/app.js"));
    assert_eq!(st, 200);
    assert_eq!(body.as_str().unwrap(), "console.log('nit')");

    for route in ["/", "/repos/1", "/changes/10"] {
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
    let (st, e) = http_get(&server.url("/api/changes/12"));
    assert_eq!(st, 404);
    assert!(e["error"].is_string());
}

#[test]
fn runs_api_only_without_web_ui() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path().join("nit.sqlite3"), None);

    let (st, health) = http_get(&server.url("/api/health"));
    assert_eq!(st, 200);
    assert_eq!(health["status"], "ok");

    // No SPA → client routes are a bare 404 (no index.html to fall back to).
    let (st, _) = http_get(&server.url("/changes/12"));
    assert_eq!(st, 404);
}

/// Everything under /api is JSON in/out — including paths axum rejects
/// before a handler runs (unknown endpoint, bad path/body types, wrong
/// method). None of it may fall through to the SPA.
#[test]
fn api_errors_are_json_everywhere() {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(dir.path().join("nit.sqlite3"), Some(&WEB_UI));

    // Unknown /api paths error as JSON, not the SPA fallback.
    for path in ["/api", "/api/", "/api/nonexistent", "/api/chain/12"] {
        let (st, body) = http_get(&server.url(path));
        assert_eq!(st, 404, "{path}: {body}");
        assert!(body["error"].is_string(), "{path}: {body}");
    }

    let (st, body) = http_get(&server.url("/api/changes/abc"));
    assert_eq!(st, 400, "{body}");
    assert!(body["error"].is_string(), "{body}");

    // A malformed JSON body errors as JSON, not text/plain.
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
