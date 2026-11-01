//! Validated document paths (ticket 0008).
//!
//! Every path from a request or the AI becomes a [`DocPath`] before the store
//! touches the filesystem. Parsing is syntactic: it never canonicalizes or
//! checks existence, because a new document does not exist yet.

use std::path::{Component, Path, PathBuf};

use crate::store::StoreError;
use crate::store::layout;

/// A relative path that is known to be a valid sdlc document.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DocPath(PathBuf);

impl DocPath {
    /// Parses and validates a raw path string.
    ///
    /// The path must be non-empty, relative, use `/` separators, contain only
    /// normal components (no `.`, `..`, root or prefix), have no empty
    /// segments or trailing slash, and match the document layout whitelist.
    /// Anything that fails, including percent-encoded traversal, returns
    /// [`StoreError::NotAllowed`].
    pub fn parse(raw: &str) -> Result<Self, StoreError> {
        let path = Path::new(raw);
        let only_normal = path
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
        let has_empty_segment = raw.split('/').any(str::is_empty);
        // `Path::components` normalizes away `.` except at the start, so a raw
        // segment check is required to reject `sdlc/PRD.md/.` and friends.
        let has_dot_segment = raw
            .split('/')
            .any(|segment| segment == "." || segment == "..");

        if raw.is_empty()
            || has_empty_segment
            || has_dot_segment
            || raw.contains('\\')
            || !only_normal
            || !layout::is_valid(path)
        {
            return Err(StoreError::NotAllowed(raw.to_owned()));
        }

        Ok(Self(path.to_path_buf()))
    }

    /// The validated relative path.
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::layout::{CoreDoc, DocKind};

    fn assert_valid(raw: &str, expected: DocKind) {
        let path = DocPath::parse(raw)
            .unwrap_or_else(|err| panic!("expected {raw:?} to be valid, got {err:?}"));

        assert_eq!(path.as_path(), Path::new(raw));
        assert_eq!(layout::kind(path.as_path()), Some(expected));
        assert!(layout::is_valid(path.as_path()));
    }

    fn assert_invalid(raw: &str) {
        match DocPath::parse(raw) {
            Err(StoreError::NotAllowed(actual)) => assert_eq!(actual, raw),
            other => panic!("expected {raw:?} to be NotAllowed, got {other:?}"),
        }
    }

    #[test]
    fn parses_every_allowed_path_with_its_kind() {
        let cases = [
            ("sdlc/PRD.md", DocKind::Core(CoreDoc::Prd)),
            ("sdlc/architecture.md", DocKind::Core(CoreDoc::Architecture)),
            ("sdlc/conventions.md", DocKind::Core(CoreDoc::Conventions)),
            ("AGENTS.md", DocKind::Core(CoreDoc::Agents)),
            ("sdlc/epics/0001-foundation/description.md", DocKind::Epic),
            (
                "sdlc/epics/0001-foundation/tickets/0001-project.md",
                DocKind::Ticket,
            ),
            ("sdlc/decisions/0003-use-axum.md", DocKind::Decision),
            ("sdlc/features/export.md", DocKind::Feature),
            ("sdlc/features/v2-api.md", DocKind::Feature),
        ];

        for (raw, expected) in cases {
            assert_valid(raw, expected);
        }
    }

    #[test]
    fn rejects_every_invalid_path_with_not_allowed() {
        let invalid = [
            "",
            ".",
            "..",
            "../AGENTS.md",
            "sdlc/../AGENTS.md",
            "./sdlc/PRD.md",
            "/etc/passwd",
            "/sdlc/PRD.md",
            "sdlc\\PRD.md",
            "C:\\sdlc\\PRD.md",
            "sdlc/PRD.md/",
            "sdlc//PRD.md",
            "sdlc/PRD.md/.",
            "sdlc/./PRD.md",
            "sdlc/PRD.md/..",
            "sdlc/unknown/PRD.md",
            "sdlc/PRD.txt",
            "sdlc/PRD.MD",
            "sdlc/PRD.md ",
            "sdlc/PRD",
            "sdlc/AGENTS.md",
            "AGENTS.md/",
            "sdlc/features/nested/export.md",
            "sdlc/features/../PRD.md",
            "sdlc/features/Export.md",
            "sdlc/features/export.MD",
            "sdlc/features/export",
            "sdlc/features/export.md/extra",
            "sdlc/epics/use-axum/description.md",
            "sdlc/epics/0001_foundation/description.md",
            "sdlc/epics/0001--foundation/description.md",
            "sdlc/epics/-0001-foundation/description.md",
            "sdlc/epics/0001-foundation/0001-project.md",
            "sdlc/epics/0001-foundation/tickets/project.md",
            "sdlc/epics/0001-foundation/tickets/0001-project.txt",
            "sdlc/epics/0001-foundation/tickets/nested/0001-project.md",
            "sdlc/decisions/use-axum.md",
            "sdlc/decisions/0003-use-axum.txt",
        ];

        for raw in invalid {
            assert_invalid(raw);
        }
    }

    #[test]
    fn rejects_encoded_traversal_and_absolute_paths() {
        for raw in [
            "%2e%2e%2f",
            "..%2f",
            "sdlc%2fPRD.md",
            "sdlc/features/../../AGENTS.md",
            "/etc/passwd",
            "C:\\sdlc\\PRD.md",
        ] {
            assert_invalid(raw);
        }
    }

    #[test]
    fn accepts_four_digit_boundaries_and_rejects_five_digits() {
        for raw in [
            "sdlc/epics/0000-foundation/description.md",
            "sdlc/epics/9999-foundation/description.md",
            "sdlc/epics/0001-foundation/tickets/0000-project.md",
            "sdlc/epics/0001-foundation/tickets/9999-project.md",
            "sdlc/decisions/0000-use-axum.md",
            "sdlc/decisions/9999-use-axum.md",
        ] {
            let path = DocPath::parse(raw)
                .unwrap_or_else(|err| panic!("expected {raw:?} to be valid, got {err:?}"));
            assert!(layout::is_valid(path.as_path()));
        }

        for raw in [
            "sdlc/epics/10000-foundation/description.md",
            "sdlc/epics/0001-foundation/tickets/10000-project.md",
            "sdlc/decisions/10000-use-axum.md",
        ] {
            assert_invalid(raw);
        }
    }

    #[test]
    fn parses_a_new_path_without_the_file_existing() {
        let raw = "sdlc/features/not-created-yet.md";
        assert!(
            !Path::new(raw).exists(),
            "the test path must not exist on disk"
        );

        assert_valid(raw, DocKind::Feature);
    }
}
