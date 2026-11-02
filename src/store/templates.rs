//! Starter templates for document creation (ticket 0015).
//!
//! The epic, decision and ticket templates are normative in `sdlc/conventions.md`
//! section 9; the feature template follows the same front matter and single H1
//! rules. Titles are rendered into the H1 only, never into the YAML front matter,
//! so a title can never inject YAML or break the block.

use crate::store::StoreError;

/// The maximum length of a validated document title, in characters.
pub const MAX_TITLE_LEN: usize = 200;

/// Renders the content of a new epic description.
///
/// The epic starts with `status: planned`, carries a numbered H1 and the
/// normative outcome, tickets and exit-criteria sections.
pub fn epic_content(number: u32, title: &str, slug: &str) -> String {
    format!(
        "---\nstatus: planned\n---\n\n# {number:04}: {title}\n\n## Outcome\nWhat this epic delivers when it is done.\n\n## Tickets\n- [{number}: {title}](tickets/{number}-{slug}.md)\n\n## Exit criteria\nObservable conditions for calling the epic done.\n"
    )
}

/// Renders the content of a new feature: planned status and the title as the
/// single H1. Features are one file, so they carry no body sections yet.
pub fn feature_content(title: &str) -> String {
    format!("---\nstatus: planned\n---\n\n# {title}\n")
}

/// Renders the content of a new decision: proposed status (ticket 0015) and the
/// numbered H1 with the context, decision and consequences sections.
pub fn decision_content(number: u32, title: &str) -> String {
    format!(
        "---\nstatus: proposed\n---\n\n# {number:04}: {title}\n\n## Context\nWhy this decision was needed.\n\n## Decision\nWhat was chosen.\n\n## Consequences\nWhat gets easier, what gets harder.\n"
    )
}

/// Renders the content of a new ticket, with an optional feature binding.
pub fn ticket_content(number: u32, title: &str, feature: Option<&str>) -> String {
    let front_matter = match feature {
        Some(feature) => format!("---\nfeature: {feature}\nstatus: todo\ndepends_on: []\n---\n\n"),
        None => "---\nstatus: todo\ndepends_on: []\n---\n\n".to_string(),
    };

    format!(
        "{front_matter}# {number:04}: {title}\n\n## Description\nWhat to build and why, in two or three sentences.\n\n## Acceptance criteria\n- Observable, testable statements.\n"
    )
}

/// Slugifies a document title: lowercase ASCII, spaces to `-`, other punctuation
/// dropped, hyphens collapsed, and the result must not be empty.
///
/// The slug is built from ASCII alphanumeric characters only, so titles carrying
/// `/`, `..`, quotes or unicode letters cannot produce a path that escapes the
/// project root.
pub fn slugify(title: &str) -> Result<String, StoreError> {
    let trimmed = title.trim();
    let mut slug = String::new();

    for word in trimmed.split_whitespace() {
        let mut word_slug = String::new();
        for character in word.chars() {
            if character.is_ascii_alphanumeric() {
                word_slug.push(character.to_ascii_lowercase());
            }
        }
        if word_slug.is_empty() {
            continue;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(&word_slug);
    }

    if slug.is_empty() {
        return Err(StoreError::InvalidTitle(trimmed.to_string()));
    }

    Ok(slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugifies_a_title_to_a_lowercase_kebab_slug() {
        assert_eq!(slugify("Add CSV export").expect("a slug"), "add-csv-export");
        assert_eq!(slugify("v2 API").expect("a slug"), "v2-api");
        assert_eq!(slugify("Add  Export").expect("a slug"), "add-export");
        assert_eq!(slugify("Add, Export!").expect("a slug"), "add-export");
    }

    #[test]
    fn slugify_keeps_only_ascii_alphanumeric_characters() {
        assert_eq!(slugify("../evil").expect("a slug"), "evil");
        assert_eq!(slugify("Ünicode").expect("a slug"), "nicode");
    }

    #[test]
    fn slugify_rejects_titles_without_a_usable_slug() {
        assert!(slugify("!!!").is_err());
        assert!(slugify("   ").is_err());
        assert!(slugify("...\"\"").is_err());
    }

    #[test]
    fn epic_content_carries_the_planned_status_and_numbered_h1() {
        let content = epic_content(7, "Add export", "add-export");

        assert!(content.starts_with("---\nstatus: planned\n---\n\n"));
        assert!(content.contains("# 0007: Add export"));
        assert!(content.contains("## Outcome"));
        assert!(content.contains("## Tickets"));
        assert!(content.contains("## Exit criteria"));
        assert!(content.ends_with("calling the epic done.\n"));
    }

    #[test]
    fn feature_content_carries_the_planned_status_and_the_h1() {
        let content = feature_content("Export");

        assert_eq!(content, "---\nstatus: planned\n---\n\n# Export\n");
    }

    #[test]
    fn decision_content_carries_the_proposed_status_and_numbered_h1() {
        let content = decision_content(9, "Use serde_yaml_ng");

        assert!(content.starts_with("---\nstatus: proposed\n---\n\n"));
        assert!(content.contains("# 0009: Use serde_yaml_ng"));
        assert!(content.contains("## Context"));
        assert!(content.contains("## Decision"));
        assert!(content.contains("## Consequences"));
    }

    #[test]
    fn ticket_content_renders_the_numbered_h1_and_optional_feature() {
        let with_feature = ticket_content(7, "Add CSV export", Some("export"));

        assert!(
            with_feature.starts_with("---\nfeature: export\nstatus: todo\ndepends_on: []\n---\n\n")
        );
        assert!(with_feature.contains("# 0007: Add CSV export"));
        assert!(with_feature.ends_with("statements.\n"));

        let without_feature = ticket_content(7, "Add CSV export", None);

        assert!(without_feature.starts_with("---\nstatus: todo\ndepends_on: []\n---\n\n"));
        assert!(!without_feature.contains("feature:"));
    }
}
