//! `nit watch`: the reviewer's entries posted to the session's inbox.
//!
//! Each test binds a stand-in for the inbox socket the harness exports,
//! and reads what the watch posts to it.

mod common;

use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};

use serde_json::Value;

use common::{
    GitRepo, HANG, TestServer, change_by_label, first_repo_id, get_changes, in_checkout, msg, nit,
    nit_env, nit_register, nit_spawn, review,
};

/// A stand-in for the session's inbox socket.
///
/// The watch opens one connection per message and closes it, so each
/// connection's lines are one message's frames.
struct Inbox {
    path: PathBuf,
    messages: Receiver<Vec<String>>,
}

impl Inbox {
    fn bind(path: PathBuf) -> Inbox {
        let listener = UnixListener::bind(&path).expect("bind the inbox");
        let (tx, messages) = channel();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let frames: Vec<String> = BufReader::new(stream)
                    .lines()
                    .map_while(Result::ok)
                    .collect();
                if tx.send(frames).is_err() {
                    return;
                }
            }
        });
        Inbox { path, messages }
    }

    /// The `nit watch` arguments that point it at this inbox.
    fn args(&self) -> [&str; 3] {
        ["watch", "--inbox", self.path.to_str().expect("utf-8 path")]
    }

    /// The next message's frames, each parsed as JSON.
    fn next(&self) -> Vec<Value> {
        let frames = self
            .messages
            .recv_timeout(HANG)
            .expect("the watch posts a message");
        frames
            .iter()
            .map(|line| serde_json::from_str(line).expect("a JSON frame"))
            .collect()
    }
}

/// The text of the message's `user` frame.
fn posted_text(frames: &[Value]) -> String {
    let user = frames
        .iter()
        .find(|f| f["type"] == "user")
        .expect("a user frame");
    user["message"]["content"]
        .as_str()
        .expect("the message text")
        .to_owned()
}

/// A fixture repo on a branch of its own, with one commit registered for
/// review, and the inbox its watch will post to.
fn session() -> (GitRepo, TestServer, Inbox, u64) {
    let g = GitRepo::new();
    let c1 = g.commit(&[g.root], &msg("one", "I001"), &[("a.txt", "a\n")]);
    g.branch("feat", c1);
    g.repo.set_head("refs/heads/feat").unwrap();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let (ok, _, err) = nit_register(&server, &g);
    assert!(ok, "push failed: {err}");
    let change_number = get_changes(&server, "")[0]["id"]
        .as_u64()
        .expect("the registered change");
    let inbox = Inbox::bind(g.dir.path().join("inbox.sock"));
    (g, server, inbox, change_number)
}

/// A second commit on the branch, pushed for review.
fn push_second(g: &GitRepo, server: &TestServer) -> u64 {
    let c2 = g.commit(&[g.tip("feat")], &msg("two", "I002"), &[("b.txt", "b\n")]);
    g.branch("feat", c2);
    let (ok, _, err) = nit(server, g, &["push"]);
    assert!(ok, "push failed: {err}");
    change_by_label(server, first_repo_id(server), "I002")["id"]
        .as_u64()
        .expect("the new change")
}

/// The author's own push produces a `revision` entry, which the watch
/// drops, so only the review reaches the inbox.
#[test]
fn watch_posts_a_review_to_the_inbox() {
    let (g, server, inbox, change_number) = session();

    let _watch = nit_spawn(&server, &g, &inbox.args(), &[]);
    review(&server, change_number, "request_changes", "fix the unwrap");

    let text = posted_text(&inbox.next());
    assert!(
        text.contains("reviewer: request_changes"),
        "posted the review: {text}"
    );
    assert!(text.contains("fix the unwrap"), "{text}");
    assert!(!text.contains("revision"), "dropped the push: {text}");
}

/// The watch reads by the checkout's tag, so a change pushed after it
/// started is covered too.
#[test]
fn watch_posts_a_review_of_a_change_pushed_after_it_started() {
    let (g, server, inbox, _) = session();

    let _watch = nit_spawn(&server, &g, &inbox.args(), &[]);
    let two = push_second(&g, &server);
    review(&server, two, "request_changes", "fix the unwrap");

    let text = posted_text(&inbox.next());
    assert!(
        text.contains("reviewer: request_changes"),
        "posted the review of the new change: {text}"
    );
}

/// The harness exports a token for a session whose own children must
/// prove themselves, and the watch opens every connection with it.
#[test]
fn watch_opens_with_the_token_the_harness_exports() {
    let (g, server, inbox, change_number) = session();

    let _watch = nit_spawn(
        &server,
        &g,
        &inbox.args(),
        &[("CLAUDE_CODE_MESSAGING_TOKEN", "t0ken")],
    );
    review(&server, change_number, "request_changes", "fix the unwrap");

    let frames = inbox.next();
    assert_eq!(frames[0]["type"], "auth", "the auth line comes first");
    assert_eq!(frames[0]["token"], "t0ken");
}

/// One watch per session: the second says so and leaves, so a review is
/// never posted twice.
#[test]
fn a_second_watch_leaves_the_session_to_the_first() {
    let (g, server, inbox, change_number) = session();

    let _watch = nit_spawn(&server, &g, &inbox.args(), &[]);
    review(&server, change_number, "request_changes", "fix the unwrap");
    // The first watch holds the lock once it has posted.
    inbox.next();

    assert_leaves_the_session(&server, &g, &inbox.args(), &[]);
}

/// A restarted session gets a new inbox but keeps its session id, and
/// its watch keeps the files of the watch before it.
#[test]
fn a_watch_on_a_new_inbox_keeps_the_session() {
    let (g, server, inbox, change_number) = session();
    let session = [("CLAUDE_CODE_SESSION_ID", "sess-1")];

    let _watch = nit_spawn(&server, &g, &by_branch(&inbox), &session);
    review(&server, change_number, "request_changes", "fix the unwrap");
    // The first watch holds the lock once it has posted.
    inbox.next();

    let restarted = Inbox::bind(g.dir.path().join("restarted.sock"));
    assert_leaves_the_session(&server, &g, &by_branch(&restarted), &session);
}

/// Runs a second watch, and checks that it leaves the session to the
/// watch that holds it.
fn assert_leaves_the_session(
    server: &TestServer,
    g: &GitRepo,
    args: &[&str],
    env: &[(&str, &str)],
) {
    let (ok, out, err) = nit_env(server, g, args, env);
    assert!(ok, "the second watch failed: {err}");
    assert_eq!(
        out.as_str(),
        Some("another nit watch already holds this session")
    );
}

/// The watch arguments for `inbox`, reading by the branch, because the
/// fixture's push carries no session tag.
fn by_branch(inbox: &Inbox) -> Vec<&str> {
    [&inbox.args()[..], &["--tag", "branch=feat"]].concat()
}

/// The watch exits when its parent exits. A watch that a hook starts must
/// exit when its session ends.
#[test]
fn the_watch_exits_when_its_parent_exits() {
    let (g, server, inbox, change_number) = session();
    // The parent is a shell that exits when its stdin closes. The watch
    // shares its stdout, so that pipe reaches EOF only once both exit.
    let mut parent = in_checkout(&mut Command::new("sh"), &server, &g)
        .args(["-c", r#""$@" & read _"#, "sh", env!("CARGO_BIN_EXE_nit")])
        .args(inbox.args())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn the parent shell");
    review(&server, change_number, "request_changes", "fix the unwrap");
    // The watch is running once it has posted.
    inbox.next();

    drop(parent.stdin.take());
    parent.wait().expect("the parent exits");
    std::io::copy(
        &mut parent.stdout.take().expect("stdout"),
        &mut std::io::sink(),
    )
    .expect("the watch exits and closes stdout");
}

/// The plugin starts a watch in every repo, so a repo that the server has
/// not registered ends the watch without output.
#[test]
fn watch_in_a_repo_the_server_has_not_registered_exits_quietly() {
    let g = GitRepo::new();
    let server = TestServer::start(g.dir.path().join("nit.sqlite3"), None);
    let inbox = Inbox::bind(g.dir.path().join("inbox.sock"));

    let (ok, out, err) = nit(&server, &g, &inbox.args());
    assert!(ok, "{err}");
    assert_eq!(out.as_str(), Some(""));
    assert_eq!(err, "");
}

/// A server that is down ends the watch without output.
#[test]
fn watch_with_the_server_down_exits_quietly() {
    let (g, server, inbox, _) = session();
    // A socket that is bound but never listens: the kernel refuses every
    // connection to its port, and no other socket can bind the port while
    // this one is open.
    let refusing = tokio::net::TcpSocket::new_v4().expect("a socket");
    refusing
        .bind("127.0.0.1:0".parse().expect("an address"))
        .expect("bind a port");
    let base = format!("http://{}", refusing.local_addr().expect("the port"));

    let out = in_checkout(&mut Command::new(env!("CARGO_BIN_EXE_nit")), &server, &g)
        .args(inbox.args())
        .env("NIT_SERVER", &base)
        .output()
        .expect("run nit watch");
    assert!(out.status.success(), "{out:?}");
    assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
}

/// With nowhere to post, the watch says so rather than watching nothing.
#[test]
fn watch_without_an_inbox_fails() {
    let (g, server, _inbox, _) = session();

    let (ok, _, err) = nit(&server, &g, &["watch"]);
    assert!(!ok, "a watch with no inbox should fail");
    assert!(err.contains("no inbox"), "{err}");
}
