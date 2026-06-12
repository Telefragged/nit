//! Change identity helpers: `Change-Id:` trailer extraction, the
//! required-Change-Id validation (docs/data-model.md "Change identity"),
//! and commit subject extraction.

use std::collections::HashMap;

/// The subject of a commit message: first paragraph, leading blank lines
/// skipped, inner newlines collapsed to spaces (git's
/// `find_commit_subject` + `format_subject`).
#[must_use]
pub fn subject_of(message: &str) -> String {
    let body = message.trim_start_matches(['\n', '\r']);
    let para = body.split("\n\n").next().unwrap_or("");
    para.replace('\n', " ").trim().to_string()
}

/// Extract the `Change-Id:` trailer value from a commit message using
/// git's trailer parser. Keys match ASCII-case-insensitively; when a
/// message (incorrectly) carries several, the last one wins.
#[must_use]
pub fn change_id_trailer(message: &str) -> Option<String> {
    let trailers = git2::message_trailers_strs(message).ok()?;
    let mut found = None;
    for (key, value) in trailers.iter() {
        if key.eq_ignore_ascii_case("Change-Id") {
            let value = value.trim();
            if !value.is_empty() {
                found = Some(value.to_string());
            }
        }
    }
    found
}

/// Change keys for the walked commits' messages (walk order, oldest
/// first), enforcing the required-Change-Id contract: every commit
/// carries a `Change-Id:` trailer, no two commits share one, and
/// `fixup!`/`squash!` commits are rejected — squash them locally before
/// pushing. `short_shas` parallels `messages` and is only used in error
/// texts.
///
/// # Errors
/// The documented scan-failure message for the first violated rule.
pub fn require_keys(messages: &[String], short_shas: &[String]) -> Result<Vec<String>, String> {
    debug_assert_eq!(messages.len(), short_shas.len());

    let fixups: Vec<&str> = messages
        .iter()
        .zip(short_shas)
        .filter(|(m, _)| {
            let subject = subject_of(m);
            subject.starts_with("fixup! ") || subject.starts_with("squash! ")
        })
        .map(|(_, sha)| sha.as_str())
        .collect();
    if !fixups.is_empty() {
        return Err(format!(
            "chain contains fixup!/squash! commits ({}) — squash them into \
             their targets before pushing",
            fixups.join(", ")
        ));
    }

    let mut keys = Vec::with_capacity(messages.len());
    let mut missing = Vec::new();
    for (message, sha) in messages.iter().zip(short_shas) {
        match change_id_trailer(message) {
            Some(token) => keys.push(token),
            None => missing.push(sha.as_str()),
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "commits without a Change-Id trailer ({}) — every commit needs one",
            missing.join(", ")
        ));
    }

    let mut seen: HashMap<&str, &str> = HashMap::new(); // token → first sha
    for (key, sha) in keys.iter().zip(short_shas) {
        if let Some(first) = seen.insert(key.as_str(), sha.as_str()) {
            return Err(format!(
                "duplicate Change-Id {key} on commits {first} and {sha} — \
                 every commit needs its own"
            ));
        }
    }
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_extraction() {
        assert_eq!(subject_of("one line\n\nbody"), "one line");
        assert_eq!(subject_of("wrapped\nsubject\n\nbody"), "wrapped subject");
        assert_eq!(subject_of("\n\nleading blank"), "leading blank");
        assert_eq!(subject_of("trailing newline\n"), "trailing newline");
        assert_eq!(subject_of(""), "");
    }

    #[test]
    fn trailer_basic() {
        let msg = "subject\n\nbody text\n\nChange-Id: I0123abcd\n";
        assert_eq!(change_id_trailer(msg), Some("I0123abcd".into()));
    }

    #[test]
    fn trailer_case_insensitive_key() {
        let msg = "s\n\nchange-id: Iaaa\n";
        assert_eq!(change_id_trailer(msg), Some("Iaaa".into()));
    }

    #[test]
    fn trailer_among_others() {
        let msg = "s\n\nbody\n\nSigned-off-by: A <a@b>\nChange-Id: Ixyz\nReviewed-by: B <b@c>\n";
        assert_eq!(change_id_trailer(msg), Some("Ixyz".into()));
    }

    #[test]
    fn trailer_last_occurrence_wins() {
        let msg = "s\n\nChange-Id: Ione\nChange-Id: Itwo\n";
        assert_eq!(change_id_trailer(msg), Some("Itwo".into()));
    }

    #[test]
    fn trailer_absent_or_not_a_trailer() {
        assert_eq!(change_id_trailer("subject only\n"), None);
        // In the body, not the trailer block.
        let msg = "s\n\nChange-Id: Inope\n\nmore body, no trailers here";
        assert_eq!(change_id_trailer(msg), None);
    }

    fn msgs(texts: &[&str]) -> Vec<String> {
        texts.iter().map(ToString::to_string).collect()
    }

    fn shas(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("sha{i}")).collect()
    }

    #[test]
    fn require_keys_happy_path() {
        let messages = msgs(&["a\n\nChange-Id: Iaaa\n", "b\n\nChange-Id: Ibbb\n"]);
        assert_eq!(
            require_keys(&messages, &shas(2)),
            Ok(vec!["Iaaa".to_string(), "Ibbb".to_string()])
        );
    }

    #[test]
    fn require_keys_rejects_fixup_and_squash_commits() {
        let messages = msgs(&[
            "a\n\nChange-Id: Iaaa\n",
            "fixup! a\n",
            "squash! a\n\nChange-Id: Ibbb\n",
        ]);
        let err = require_keys(&messages, &shas(3)).expect_err("should be rejected");
        assert!(err.contains("fixup!/squash!"), "{err}");
        assert!(err.contains("sha1") && err.contains("sha2"), "{err}");
    }

    #[test]
    fn require_keys_rejects_missing_trailer() {
        let messages = msgs(&["a\n\nChange-Id: Iaaa\n", "no trailer\n"]);
        let err = require_keys(&messages, &shas(2)).expect_err("should be rejected");
        assert!(err.contains("without a Change-Id trailer"), "{err}");
        assert!(err.contains("sha1") && !err.contains("sha0"), "{err}");
    }

    #[test]
    fn require_keys_rejects_duplicate_trailer() {
        let messages = msgs(&[
            "a\n\nChange-Id: Idup\n",
            "b\n\nChange-Id: Ibbb\n",
            "c\n\nChange-Id: Idup\n",
        ]);
        let err = require_keys(&messages, &shas(3)).expect_err("should be rejected");
        assert!(err.contains("duplicate Change-Id Idup"), "{err}");
        assert!(err.contains("sha0") && err.contains("sha2"), "{err}");
    }
}
