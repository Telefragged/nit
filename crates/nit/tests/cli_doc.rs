mod common;

use std::io::Write;
use std::process::Stdio;

use common::{GitRepo, TestServer, nit_command, post, replica};
use nit_document::Writer;

fn doc(
    server: &TestServer,
    g: &GitRepo,
    args: &[&str],
    stdin: Option<&str>,
) -> (bool, String, String) {
    let mut child = nit_command(server, g, &[&["doc"], args].concat())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("running nit");
    let mut pipe = child.stdin.take().expect("stdin");
    pipe.write_all(stdin.unwrap_or_default().as_bytes())
        .expect("write stdin");
    drop(pipe);
    let out = child.wait_with_output().expect("nit exits");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn ok(server: &TestServer, g: &GitRepo, args: &[&str], stdin: Option<&str>) -> String {
    let (ok, out, err) = doc(server, g, args, stdin);
    assert!(ok, "nit doc {args:?} failed: {err}");
    out
}

fn show(server: &TestServer, g: &GitRepo, since: Option<&str>) -> (String, String) {
    let mut args = vec!["show", "1"];
    args.extend(since.into_iter().flat_map(|version| ["--since", version]));
    let (ok, out, err) = doc(server, g, &args, None);
    assert!(ok, "nit doc {args:?} failed: {err}");
    let version = err
        .trim_end()
        .strip_prefix("document 1  version ")
        .unwrap_or_else(|| panic!("no version in: {err}"));
    let version = version.split("  since ").next().expect("a version");
    (out, version.to_owned())
}

fn server_and_repo() -> (GitRepo, TestServer) {
    let g = GitRepo::new();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    (g, server)
}

#[test]
fn the_author_writes_a_document() {
    let (g, server) = server_and_repo();
    let created = ok(
        &server,
        &g,
        &["new", "--title", "Plan", "-F", "-"],
        Some("# Title\n\nIntro."),
    );
    assert!(created.starts_with("document 1  http"), "{created}");
    assert!(created.trim_end().ends_with("/documents/1"), "{created}");
    assert!(ok(&server, &g, &["list"], None).contains("Plan"));
    assert_eq!(show(&server, &g, None).0, "# Title\n\nIntro.\n");

    ok(
        &server,
        &g,
        &["append", "1", "-F", "-"],
        Some("[change 5](/changes/5)\n"),
    );
    ok(&server, &g, &["append", "1", "-F", "-"], Some("End."));
    assert_eq!(
        show(&server, &g, None).0,
        "# Title\n\nIntro.\n\n[change 5](/changes/5)\n\nEnd.\n"
    );

    ok(
        &server,
        &g,
        &["edit", "1", "--old", "Intro", "--new", "Summary"],
        None,
    );
    ok(
        &server,
        &g,
        &["edit", "1", "--old", "End.", "-F", "-"],
        Some("The\nend.\n"),
    );
    assert_eq!(
        show(&server, &g, None).0,
        "# Title\n\nSummary.\n\n[change 5](/changes/5)\n\nThe\nend.\n"
    );
}

#[test]
fn edit_requires_one_occurrence_of_the_old_text() {
    let (g, server) = server_and_repo();
    ok(
        &server,
        &g,
        &["new", "--title", "Plan", "-F", "-"],
        Some("one two two\n"),
    );
    let edit = |old: &str| {
        doc(
            &server,
            &g,
            &["edit", "1", "--old", old, "--new", "x"],
            None,
        )
    };

    let (replaced, _, err) = edit("three");
    assert!(!replaced && err.contains("does not occur"), "{err}");
    let (replaced, _, err) = edit("two");
    assert!(!replaced && err.contains("occurs 2 times"), "{err}");
    let (replaced, _, err) = doc(&server, &g, &["edit", "1", "--old", "one"], None);
    assert!(!replaced && err.contains("--new"), "{err}");
    assert_eq!(show(&server, &g, None).0, "one two two\n");
}

#[test]
fn show_since_prints_the_hunks_with_their_writers() {
    let (g, server) = server_and_repo();
    ok(
        &server,
        &g,
        &["new", "--title", "Plan", "-F", "-"],
        Some("One.\nTwo.\nThree.\n"),
    );
    let (_, before) = show(&server, &g, None);

    let mut reviewer = replica(&server, 1);
    let start = reviewer.version();
    reviewer
        .editor(Writer::Reviewer)
        .replace("Three.\n", "Three.\nA question.\n")
        .unwrap();
    let (st, res) = post(&server, 1, &reviewer, &start);
    assert_eq!(st, 200, "{res}");
    ok(
        &server,
        &g,
        &["edit", "1", "--old", "One.", "--new", "First."],
        None,
    );

    let (edits, version) = show(&server, &g, Some(&before));
    assert_eq!(
        edits,
        "@@ -0,1 +0,1 @@ by author\n-One.\n+First.\n@@ -3,0 +3,1 @@ by reviewer\n+A question.\n"
    );
    assert_ne!(version, before);
    assert_eq!(show(&server, &g, Some(&version)).0, "");
}
