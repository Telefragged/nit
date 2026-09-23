//! `nit watch`: the reviewer's entries posted to the session's inbox.
//!
//! Each test binds a stand-in for the inbox socket the harness exports,
//! and reads what the watch posts to it.

mod common;

use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use serde_json::Value;

use common::{
    GitRepo, HANG, TestServer, change_by_label, first_repo_id, get_changes, msg, nit, nit_register,
    nit_spawn, review,
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
    fn bind(dir: &Path) -> Inbox {
        let path = dir.join("inbox.sock");
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
    let inbox = Inbox::bind(g.dir.path());
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

    let (ok, out, err) = nit(&server, &g, &inbox.args());
    assert!(ok, "the second watch failed: {err}");
    assert_eq!(
        out.as_str(),
        Some("another nit watch already holds this session")
    );
}

/// With nowhere to post, the watch says so rather than watching nothing.
#[test]
fn watch_without_an_inbox_fails() {
    let (g, server, _inbox, _) = session();

    let (ok, _, err) = nit(&server, &g, &["watch"]);
    assert!(!ok, "a watch with no inbox should fail");
    assert!(err.contains("no inbox"), "{err}");
}
