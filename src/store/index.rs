//! The PRD feature-index markers (scaffolded by ticket 0017, written by ticket 0028).
//!
//! The markers are HTML comments, invisible when rendered, and delimit the
//! region of `sdlc/PRD.md` that the index writer rewrites whenever a feature is
//! created, renamed, deleted or changes status.

use crate::store::DocEntry;

/// The marker that opens the PRD feature index region.
pub const FEATURE_INDEX_START: &str = "<!-- sdlc:feature-index:start -->";

/// The marker that closes the PRD feature index region.
pub const FEATURE_INDEX_END: &str = "<!-- sdlc:feature-index:end -->";

/// The status a feature renders with when its front matter carries none.
const DEFAULT_STATUS: &str = "planned";

/// The outcome of splicing the rendered feature index into a PRD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewriteOutcome {
    /// The PRD was rewritten around the rendered feature index.
    Updated(String),
    /// The PRD carried no usable marker pair, so it was left untouched.
    NoMarkers,
}

/// Renders the PRD feature index: one line per feature, sorted by slug and
/// terminated by a newline each, so an empty feature list renders to `""` and no
/// stale line can survive.
///
/// Each line is `- [Title](features/slug.md) — status`. Titles are escaped so
/// brackets cannot break the Markdown link, and a missing status renders as
/// `planned`.
pub fn render(features: &[DocEntry]) -> String {
    let mut sorted: Vec<&DocEntry> = features.iter().collect();
    sorted.sort_by_key(|entry| feature_slug(entry));

    let mut rendered = String::new();
    for entry in sorted {
        let slug = feature_slug(entry);
        rendered.push_str(&format!(
            "- [{}](features/{}.md) — {}\n",
            escape_title(&entry.title),
            slug,
            status_of(entry),
        ));
    }

    rendered
}

/// Rewrites the marked region of a PRD with the rendered feature index.
///
/// The text before the opening marker and after the closing marker is preserved
/// byte-for-byte, and the generated lines use the PRD's own line ending. The
/// first start marker and the first end marker after it delimit the region; a
/// duplicate start marker is reported and the first one wins. Returns
/// [`RewriteOutcome::NoMarkers`] when the pair is missing, leaving the input
/// untouched.
pub fn rewrite(prd: &str, entries: &[DocEntry]) -> RewriteOutcome {
    let Some((start, start_end)) = find_marker(prd, FEATURE_INDEX_START, 0) else {
        return RewriteOutcome::NoMarkers;
    };
    let Some((end, end_end)) = find_marker(prd, FEATURE_INDEX_END, start_end) else {
        return RewriteOutcome::NoMarkers;
    };
    if find_marker(prd, FEATURE_INDEX_START, end).is_some() {
        tracing::warn!(marker = %FEATURE_INDEX_START, "duplicate feature index start marker; using the first");
    }

    let before = &prd[..start];
    let start_line = &prd[start..start_end];
    let end_line = &prd[end..end_end];
    let after = &prd[end_end..];
    let eol = if prd.contains("\r\n") { "\r\n" } else { "\n" };
    let mut generated = render(entries);
    if eol == "\r\n" {
        generated = generated.replace('\n', "\r\n");
    }

    RewriteOutcome::Updated(format!(
        "{before}{start_line}{eol}{generated}{end_line}{after}"
    ))
}

/// The slug of a feature entry, taken from its validated document path.
fn feature_slug(entry: &DocEntry) -> String {
    let file_name = entry
        .path
        .as_path()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    file_name
        .strip_suffix(".md")
        .unwrap_or(file_name.as_str())
        .to_string()
}

/// The front matter status of a feature, defaulting to `planned` when absent.
fn status_of(entry: &DocEntry) -> &str {
    entry.status.as_deref().unwrap_or(DEFAULT_STATUS)
}

/// The link text of a feature line, with brackets and backslashes escaped so a
/// title cannot break the Markdown link, and newlines stripped.
fn escape_title(title: &str) -> String {
    let mut escaped = String::with_capacity(title.len());
    for character in title.chars() {
        if character == '\n' || character == '\r' {
            continue;
        }
        match character {
            '\\' => escaped.push_str("\\\\"),
            '[' => escaped.push_str("\\["),
            ']' => escaped.push_str("\\]"),
            _ => escaped.push(character),
        }
    }

    escaped
}

/// Finds the first line at or after `from` whose trimmed content equals
/// `marker`, returning the byte range of the line without its terminator.
fn find_marker(content: &str, marker: &str, from: usize) -> Option<(usize, usize)> {
    let mut line_start = content[..from.min(content.len())]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);

    while line_start < content.len() {
        let line_end = line_start
            + content[line_start..]
                .find('\n')
                .unwrap_or(content.len() - line_start);
        let line = content[line_start..line_end].trim_end_matches('\r');
        if line.trim() == marker {
            // The range covers the line text only, without its line ending.
            return Some((line_start, line_start + line.len()));
        }
        line_start = line_end + 1;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{DocKind, DocPath};

    fn feature(title: &str, status: Option<&str>, slug: &str) -> DocEntry {
        DocEntry {
            path: DocPath::parse(&format!("sdlc/features/{slug}.md")).expect("valid feature path"),
            kind: DocKind::Feature,
            title: title.to_string(),
            status: status.map(|status| status.to_string()),
            epic: None,
            feature: None,
        }
    }

    #[test]
    fn feature_index_markers_are_the_exact_contract_strings() {
        // The scaffold writes the region between these exact strings, and the
        // index writer of ticket 0028 rewrites from the same markers.
        assert_eq!(FEATURE_INDEX_START, "<!-- sdlc:feature-index:start -->");
        assert_eq!(FEATURE_INDEX_END, "<!-- sdlc:feature-index:end -->");
        assert_ne!(FEATURE_INDEX_START, FEATURE_INDEX_END);
    }

    #[test]
    fn render_returns_one_line_per_feature_sorted_by_slug() {
        let entries = [
            feature("Export", Some("shipped"), "export"),
            feature("Add CSV export", Some("in-progress"), "add-csv-export"),
        ];

        assert_eq!(
            render(&entries),
            "- [Add CSV export](features/add-csv-export.md) — in-progress\n- [Export](features/export.md) — shipped\n"
        );
    }

    #[test]
    fn render_returns_an_empty_index_for_an_empty_feature_list() {
        assert_eq!(render(&[]), "");
    }

    #[test]
    fn render_defaults_a_missing_status_to_planned() {
        let entry = feature("Export", None, "export");

        assert_eq!(
            render(&[entry]),
            "- [Export](features/export.md) — planned\n"
        );
    }

    #[test]
    fn render_escapes_title_brackets_and_backslashes() {
        let entry = feature(
            "Add [CSV] export \\ Turbo",
            Some("planned"),
            "add-csv-export",
        );

        assert_eq!(
            render(&[entry]),
            "- [Add \\[CSV\\] export \\\\ Turbo](features/add-csv-export.md) — planned\n"
        );
    }

    #[test]
    fn rewrite_fills_the_marked_region_of_the_prd() {
        let prd = "# Product Requirements Document\n\n## Purpose\n\nTODO: write the purpose.\n\n<!-- sdlc:feature-index:start -->\n<!-- sdlc:feature-index:end -->\n";
        let entry = feature("Export", Some("shipped"), "export");

        assert_eq!(
            rewrite(prd, &[entry]),
            RewriteOutcome::Updated(
                "# Product Requirements Document\n\n## Purpose\n\nTODO: write the purpose.\n\n<!-- sdlc:feature-index:start -->\n- [Export](features/export.md) — shipped\n<!-- sdlc:feature-index:end -->\n".to_string(),
            ),
        );
    }

    #[test]
    fn rewrite_is_idempotent() {
        let prd = "# Product Requirements Document\n\n## Purpose\n\nTODO: write the purpose.\n\n<!-- sdlc:feature-index:start -->\n<!-- sdlc:feature-index:end -->\n";
        let entries = [feature("Export", Some("shipped"), "export")];
        let RewriteOutcome::Updated(first) = rewrite(prd, &entries) else {
            panic!("the rewrite should update");
        };
        let RewriteOutcome::Updated(second) = rewrite(&first, &entries) else {
            panic!("the second rewrite should update");
        };

        assert_eq!(first, second);
    }

    #[test]
    fn rewrite_with_an_empty_feature_list_clears_the_region() {
        let prd = "# Title\n\n<!-- sdlc:feature-index:start -->\n- [Old export](features/old-export.md) — shipped\n<!-- sdlc:feature-index:end -->\n";

        assert_eq!(
            rewrite(prd, &[]),
            RewriteOutcome::Updated(
                "# Title\n\n<!-- sdlc:feature-index:start -->\n<!-- sdlc:feature-index:end -->\n"
                    .to_string()
            ),
        );
    }

    #[test]
    fn rewrite_without_markers_returns_no_markers() {
        let without_end = "# Title\n\n<!-- sdlc:feature-index:start -->\n";

        assert_eq!(
            rewrite(
                "# Title\n\nno markers at all\n",
                &[feature("Export", Some("shipped"), "export")]
            ),
            RewriteOutcome::NoMarkers
        );
        assert_eq!(
            rewrite(without_end, &[feature("Export", Some("shipped"), "export")]),
            RewriteOutcome::NoMarkers
        );
    }

    #[test]
    fn rewrite_keeps_crlf_line_endings_and_the_text_outside_the_region() {
        let prd = "# Title\r\n\r\nSome text.\r\n\r\n<!-- sdlc:feature-index:start -->\r\n- stale\r\n<!-- sdlc:feature-index:end -->\r\n";
        let entry = feature("Export", Some("shipped"), "export");

        let RewriteOutcome::Updated(updated) = rewrite(prd, &[entry]) else {
            panic!("the rewrite should update");
        };

        assert!(updated.starts_with("# Title\r\n\r\nSome text.\r\n\r\n"));
        assert!(updated.contains("- [Export](features/export.md) — shipped\r\n"));
        assert!(!updated.contains("- [Export](features/export.md) — shipped\n"));
        assert!(updated.ends_with("<!-- sdlc:feature-index:end -->\r\n"));
    }

    #[test]
    fn rewrite_keeps_the_text_after_the_region_without_a_trailing_newline() {
        let prd = "<!-- sdlc:feature-index:start -->\n- stale\n<!-- sdlc:feature-index:end -->";
        let entry = feature("Export", Some("shipped"), "export");

        let RewriteOutcome::Updated(updated) = rewrite(prd, &[entry]) else {
            panic!("the rewrite should update");
        };

        assert_eq!(
            updated,
            "<!-- sdlc:feature-index:start -->\n- [Export](features/export.md) — shipped\n<!-- sdlc:feature-index:end -->"
        );
    }

    #[test]
    fn rewrite_reads_markers_at_the_edges_of_the_file() {
        let prd = "<!-- sdlc:feature-index:start -->\n- stale\n<!-- sdlc:feature-index:end -->";
        let entry = feature("Export", Some("shipped"), "export");

        let RewriteOutcome::Updated(updated) = rewrite(prd, &[entry]) else {
            panic!("the rewrite should update");
        };

        assert_eq!(
            updated,
            "<!-- sdlc:feature-index:start -->\n- [Export](features/export.md) — shipped\n<!-- sdlc:feature-index:end -->"
        );
    }

    #[test]
    fn rewrite_tolerates_surrounding_spaces_on_markers() {
        let prd = "# Title\n\n  <!-- sdlc:feature-index:start -->  \n- stale\n<!-- sdlc:feature-index:end -->\n";
        let entry = feature("Export", Some("shipped"), "export");

        let RewriteOutcome::Updated(updated) = rewrite(prd, &[entry]) else {
            panic!("the rewrite should update");
        };

        assert_eq!(
            updated,
            "# Title\n\n  <!-- sdlc:feature-index:start -->  \n- [Export](features/export.md) — shipped\n<!-- sdlc:feature-index:end -->\n"
        );
    }

    #[test]
    fn rewrite_uses_the_first_start_marker_of_duplicate_markers() {
        let prd = "<!-- sdlc:feature-index:start -->\nstale\n<!-- sdlc:feature-index:start -->\nstale2\n<!-- sdlc:feature-index:end -->\n";
        let entry = feature("Export", Some("shipped"), "export");

        let RewriteOutcome::Updated(updated) = rewrite(prd, &[entry]) else {
            panic!("the rewrite should update");
        };

        assert_eq!(
            updated,
            "<!-- sdlc:feature-index:start -->\n- [Export](features/export.md) — shipped\n<!-- sdlc:feature-index:end -->\n"
        );
    }
}
