//! Change identity helpers: `Change-Id:` trailer extraction and the
//! derived-key scheme for duplicated trailers (docs/data-model.md "Change
//! identity", rule 1).

use std::collections::HashMap;

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

/// Assign effective change keys for the walked regular commits' trailers
/// (walk order, oldest first). The first commit carrying a token keeps it;
/// later duplicates get derived keys `<token>#2`, `<token>#3`, … plus a
/// scan warning each. `short_shas` parallels `trailers` and is only used
/// in warning texts.
#[must_use]
pub fn assign_trailer_keys(
    trailers: &[Option<String>],
    short_shas: &[String],
) -> (Vec<Option<String>>, Vec<String>) {
    debug_assert_eq!(trailers.len(), short_shas.len());
    let mut seen: HashMap<&str, u32> = HashMap::new();
    let mut keys = Vec::with_capacity(trailers.len());
    let mut warnings = Vec::new();
    for (trailer, short_sha) in trailers.iter().zip(short_shas) {
        keys.push(trailer.as_deref().map(|token| {
            let n = seen.entry(token).or_insert(0);
            *n += 1;
            if *n == 1 {
                token.to_string()
            } else {
                let derived = format!("{token}#{n}");
                warnings.push(format!(
                    "duplicate Change-Id {token}: commit {short_sha} tracked as {derived}"
                ));
                derived
            }
        }));
    }
    (keys, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn derived_keys_for_duplicates() {
        let trailers = vec![
            Some("Iaaa".to_string()),
            None,
            Some("Iaaa".to_string()),
            Some("Ibbb".to_string()),
            Some("Iaaa".to_string()),
        ];
        let shas: Vec<String> = (0..5).map(|i| format!("sha{i}")).collect();
        let (keys, warnings) = assign_trailer_keys(&trailers, &shas);
        assert_eq!(
            keys,
            vec![
                Some("Iaaa".to_string()),
                None,
                Some("Iaaa#2".to_string()),
                Some("Ibbb".to_string()),
                Some("Iaaa#3".to_string()),
            ]
        );
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].contains("Iaaa") && warnings[0].contains("sha2"));
        assert!(warnings[1].contains("Iaaa#3"));
    }
}
