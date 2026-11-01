//! The fixed document layout (ticket 0008).
//!
//! This module is the single source of truth for which paths are documents.
//! Validation is purely syntactic: it never touches the filesystem, so a path
//! for a document that does not exist yet is still valid.

use std::path::{Component, Path};

/// A document that lives at a fixed path in the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreDoc {
    /// The product requirements document, `sdlc/PRD.md`.
    Prd,
    /// The architecture document, `sdlc/architecture.md`.
    Architecture,
    /// The coding conventions document, `sdlc/conventions.md`.
    Conventions,
    /// The root agent instructions, `AGENTS.md`.
    Agents,
}

/// The kind of document a valid path identifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocKind {
    /// One of the fixed documents.
    Core(CoreDoc),
    /// An epic description, `sdlc/epics/NNNN-slug/description.md`.
    Epic,
    /// A feature, `sdlc/features/slug.md`.
    Feature,
    /// A decision, `sdlc/decisions/NNNN-slug.md`.
    Decision,
    /// A ticket, `sdlc/epics/NNNN-slug/tickets/NNNN-slug.md`.
    Ticket,
}

/// Reports whether `path` is exactly one of the allowed document paths.
pub fn is_valid(path: &Path) -> bool {
    kind(path).is_some()
}

/// Identifies the document kind of `path`, or `None` when it is not allowed.
///
/// The path must consist only of normal components; callers that already hold
/// a [`crate::store::DocPath`] have that guarantee.
pub fn kind(path: &Path) -> Option<DocKind> {
    let segments = normal_segments(path)?;

    match segments.as_slice() {
        ["AGENTS.md"] => Some(DocKind::Core(CoreDoc::Agents)),
        ["sdlc", "PRD.md"] => Some(DocKind::Core(CoreDoc::Prd)),
        ["sdlc", "architecture.md"] => Some(DocKind::Core(CoreDoc::Architecture)),
        ["sdlc", "conventions.md"] => Some(DocKind::Core(CoreDoc::Conventions)),
        ["sdlc", "epics", epic, "description.md"] if is_numbered_slug(epic) => Some(DocKind::Epic),
        ["sdlc", "epics", epic, "tickets", ticket]
            if is_numbered_slug(epic) && is_numbered_markdown(ticket) =>
        {
            Some(DocKind::Ticket)
        }
        ["sdlc", "decisions", decision] if is_numbered_markdown(decision) => {
            Some(DocKind::Decision)
        }
        ["sdlc", "features", feature] if is_markdown_slug(feature) => Some(DocKind::Feature),
        _ => None,
    }
}

/// Splits `path` into UTF-8 normal components, rejecting everything else.
///
/// Empty input yields `None`, so a bare `Path::new("")` is never valid.
fn normal_segments(path: &Path) -> Option<Vec<&str>> {
    let mut segments = Vec::new();

    for component in path.components() {
        match component {
            Component::Normal(segment) => segments.push(segment.to_str()?),
            _ => return None,
        }
    }

    (!segments.is_empty()).then_some(segments)
}

/// Matches a lowercase kebab-case name: alphanumeric groups joined by single
/// hyphens, with no leading, trailing or repeated hyphen.
fn is_kebab(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

/// Matches a four-digit prefix followed by a kebab-case slug, `NNNN-slug`.
///
/// The prefix must be exactly four digits: `10000-slug` fails rather than
/// being silently truncated to a five-digit name.
fn is_numbered_slug(value: &str) -> bool {
    let Some((prefix, slug)) = value.split_once('-') else {
        return false;
    };

    prefix.len() == 4 && prefix.bytes().all(|byte| byte.is_ascii_digit()) && is_kebab(slug)
}

/// Matches a numbered Markdown file, `NNNN-slug.md`.
fn is_numbered_markdown(value: &str) -> bool {
    markdown_stem(value).is_some_and(is_numbered_slug)
}

/// Matches a plain Markdown file whose stem is a kebab-case slug.
fn is_markdown_slug(value: &str) -> bool {
    markdown_stem(value).is_some_and(is_kebab)
}

/// Returns the file stem when `value` ends in the exact lowercase `.md`
/// extension and has something before it.
fn markdown_stem(value: &str) -> Option<&str> {
    value.strip_suffix(".md").filter(|stem| !stem.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_core_documents() {
        assert_eq!(
            kind(Path::new("sdlc/PRD.md")),
            Some(DocKind::Core(CoreDoc::Prd))
        );
        assert_eq!(
            kind(Path::new("sdlc/architecture.md")),
            Some(DocKind::Core(CoreDoc::Architecture))
        );
        assert_eq!(
            kind(Path::new("sdlc/conventions.md")),
            Some(DocKind::Core(CoreDoc::Conventions))
        );
        assert_eq!(
            kind(Path::new("AGENTS.md")),
            Some(DocKind::Core(CoreDoc::Agents))
        );
    }

    #[test]
    fn identifies_epic_description_and_nested_ticket() {
        assert_eq!(
            kind(Path::new("sdlc/epics/0001-foundation/description.md")),
            Some(DocKind::Epic)
        );
        assert_eq!(
            kind(Path::new(
                "sdlc/epics/0001-foundation/tickets/0001-project.md"
            )),
            Some(DocKind::Ticket)
        );
    }

    #[test]
    fn rejects_non_normal_components_directly() {
        assert!(!is_valid(Path::new("../AGENTS.md")));
        assert!(!is_valid(Path::new("/sdlc/PRD.md")));
        assert!(!is_valid(Path::new("./sdlc/PRD.md")));
    }

    #[test]
    fn rejects_nested_features_and_tickets_outside_the_tickets_directory() {
        assert!(!is_valid(Path::new("sdlc/features/nested/export.md")));
        assert!(!is_valid(Path::new(
            "sdlc/epics/0001-foundation/0001-project.md"
        )));
        assert!(!is_valid(Path::new(
            "sdlc/epics/0001-foundation/nested/tickets/0001-project.md"
        )));
    }

    #[test]
    fn rejects_unnumbered_epics_decisions_and_tickets() {
        assert!(!is_valid(Path::new("sdlc/epics/use-axum/description.md")));
        assert!(!is_valid(Path::new("sdlc/decisions/use-axum.md")));
        assert!(!is_valid(Path::new(
            "sdlc/epics/0001-foundation/tickets/project.md"
        )));
    }

    #[test]
    fn accepts_four_digit_boundaries_and_rejects_five_digits() {
        assert!(is_valid(Path::new(
            "sdlc/epics/0000-foundation/description.md"
        )));
        assert!(is_valid(Path::new(
            "sdlc/epics/9999-foundation/description.md"
        )));
        assert!(!is_valid(Path::new(
            "sdlc/epics/10000-foundation/description.md"
        )));

        assert!(is_valid(Path::new(
            "sdlc/epics/0001-foundation/tickets/0000-project.md"
        )));
        assert!(is_valid(Path::new(
            "sdlc/epics/0001-foundation/tickets/9999-project.md"
        )));
        assert!(!is_valid(Path::new(
            "sdlc/epics/0001-foundation/tickets/10000-project.md"
        )));

        assert!(is_valid(Path::new("sdlc/decisions/0000-use-axum.md")));
        assert!(is_valid(Path::new("sdlc/decisions/9999-use-axum.md")));
        assert!(!is_valid(Path::new("sdlc/decisions/10000-use-axum.md")));
    }
}
