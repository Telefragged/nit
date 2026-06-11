//! Static UI serving (docs/api.md "Static UI"): the built SPA outside
//! /api, index.html fallback for client-side routes, API-only without a
//! web dist.

mod common;

use common::{TestServer, http_get};

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

    // Client-side routes fall back to index.html.
    for route in ["/", "/chains/1", "/changes/10"] {
        let (st, body) = http_get(&server.url(route));
        assert_eq!(st, 200, "{route}");
        assert_eq!(body.as_str().unwrap(), "<html>nit-spa</html>", "{route}");
    }

    // /api routes win over the SPA.
    let (st, health) = http_get(&server.url("/api/health"));
    assert_eq!(st, 200);
    assert_eq!(health["status"], "ok");
    assert_eq!(health["version"], env!("CARGO_PKG_VERSION"));
    let (st, e) = http_get(&server.url("/api/chains/1"));
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

    let (st, _) = http_get(&server.url("/chains/1"));
    assert_eq!(st, 404);
}
