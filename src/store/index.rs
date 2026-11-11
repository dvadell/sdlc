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
/// brackets cannot break the Markdown link, line breaks become a space, and a
/// missing or unexpected status renders as `planned`. Entries whose feature
/// slug is not a valid lowercase kebab-case slug are skipped with a warning, so
/// they cannot silently vanish from the generated index.
pub fn render(features: &[DocEntry]) -> String {
    let mut sorted: Vec<(&DocEntry, String)> = Vec::new();
    for entry in features {
        let Some(slug) = feature_slug(entry) else {
            tracing::warn!(
                title = %entry.title,
                "skipping feature entry without a valid kebab-case slug"
            );
            continue;
        };
        sorted.push((entry, slug));
    }
    sorted.sort_by(|left, right| left.1.cmp(&right.1));

    let mut rendered = String::new();
    for (entry, slug) in sorted {
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
/// byte-for-byte, and the generated lines use the line ending of the opening
/// marker line, falling back to the preceding line's ending when the marker line
/// has none of its own. The first start marker and the first end marker after it
/// delimit the region; a marker after the first one is a duplicate or a stray,
/// and the first one wins. Returns [`RewriteOutcome::NoMarkers`] when the pair
/// is missing, leaving the input untouched.
pub fn rewrite(prd: &str, entries: &[DocEntry]) -> RewriteOutcome {
    let Some((start, start_end)) = find_marker(prd, FEATURE_INDEX_START, 0) else {
        return RewriteOutcome::NoMarkers;
    };
    let Some((end, end_end)) = find_marker(prd, FEATURE_INDEX_END, start_end) else {
        return RewriteOutcome::NoMarkers;
    };

    // Any marker after the first one is a duplicate or a stray, whether it sits
    // inside the region or past the closing marker; the first one wins. The
    // scans start past the marker line that was just found, so a marker is never
    // compared against its own line.
    if find_marker(prd, FEATURE_INDEX_START, start_end + 1).is_some() {
        tracing::warn!(marker = %FEATURE_INDEX_START, "duplicate feature index start marker; using the first");
    }
    if find_marker(prd, FEATURE_INDEX_END, end_end + 1).is_some() {
        tracing::warn!(marker = %FEATURE_INDEX_END, "duplicate feature index end marker; using the first");
    }

    let before = &prd[..start];
    let start_line = &prd[start..start_end];
    let end_line = &prd[end..end_end];
    let after = &prd[end_end..];
    let eol = line_ending(prd, start);
    let rendered = render(entries);
    // `render` emits one `\n`-terminated line per feature, so switching the
    // terminator only rewrites line endings, never content.
    let generated = if eol == "\n" {
        rendered
    } else {
        rendered.replace('\n', eol)
    };

    RewriteOutcome::Updated(format!(
        "{before}{start_line}{eol}{generated}{end_line}{after}"
    ))
}

/// The line ending of the line that opens with the marker at or after `marker`:
/// the marker's own terminator, or the preceding line's ending when the marker
/// line has none of its own.
fn line_ending(prd: &str, marker: usize) -> &str {
    let line_start = line_start_of(prd, marker);
    let (content_end, next_start) = line_bounds(prd, line_start);

    if next_start > content_end {
        // The marker line ends with its own terminator: the first CR or LF of
        // the line, so a bare CR is the terminator itself and a CR directly
        // followed by LF is a single CRLF pair, not a bare CR plus a later LF.
        return &prd[content_end..next_start];
    }

    // The marker line carries no terminator of its own, so it opens the last
    // line of the document; reuse the preceding line's ending instead of
    // assuming LF, so a CRLF document never mixes endings.
    let before = &prd[..line_start];
    let mut last_terminator: Option<(usize, usize)> = None;
    let mut previous: Option<char> = None;
    for (index, character) in before.char_indices() {
        // Only the LF of a CRLF pair is the pair's continuation, so only that
        // LF is suppressed; a bare CR is a terminator of its own, even when it
        // directly follows another bare CR.
        let is_lf_of_crlf = character == '\n' && previous == Some('\r');
        if (character == '\n' || character == '\r') && !is_lf_of_crlf {
            let end = if character == '\r' && before.as_bytes().get(index + 1) == Some(&b'\n') {
                index + 2
            } else {
                index + character.len_utf8()
            };
            last_terminator = Some((index, end));
        }
        previous = Some(character);
    }
    let Some((terminator_start, terminator_end)) = last_terminator else {
        // The marker line opens the first line of the document.
        return "\n";
    };
    &prd[terminator_start..terminator_end]
}

/// The slug of a feature entry, taken from its validated document path and
/// expected to already be a lowercase kebab-case slug.
///
/// The derived slug is validated before it reaches a rendered line, the same
/// way a title is escaped, so an entry that does not name a usable feature slug
/// cannot distort the generated Markdown.
fn feature_slug(entry: &DocEntry) -> Option<String> {
    let file_name = entry
        .path
        .as_path()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let slug = file_name.strip_suffix(".md").unwrap_or(file_name.as_str());
    is_kebab_slug(slug).then(|| slug.to_string())
}

/// Reports whether `value` is a lowercase kebab-case slug: alphanumeric
/// groups joined by single hyphens, with no leading, trailing or repeated
/// hyphen.
fn is_kebab_slug(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

/// The front matter status of a feature, normalized to the validated feature
/// status vocabulary and defaulting to `planned` when absent or unexpected, so
/// a hand-built entry cannot inject line breaks or unknown statuses into the
/// rendered index line.
fn status_of(entry: &DocEntry) -> &str {
    let status = entry.status.as_deref().unwrap_or(DEFAULT_STATUS);
    let normalized = status.trim();
    if normalized.is_empty() || !is_known_status(normalized) {
        DEFAULT_STATUS
    } else {
        normalized
    }
}

/// Reports whether `status` is one of the validated feature statuses.
fn is_known_status(status: &str) -> bool {
    matches!(status, "planned" | "in-progress" | "shipped")
}

/// The link text of a feature line, with brackets and backslashes escaped so a
/// title cannot break the Markdown link, and line breaks replaced by a space.
fn escape_title(title: &str) -> String {
    let mut escaped = String::with_capacity(title.len());
    let mut previous: Option<char> = None;
    for character in title.chars() {
        let continuation_of_break = character == '\n' && previous == Some('\r');
        match character {
            '\\' => escaped.push_str("\\\\"),
            '[' => escaped.push_str("\\["),
            ']' => escaped.push_str("\\]"),
            '\n' | '\r' => {
                // A CRLF pair is one line break, so it collapses to a single space.
                if !continuation_of_break {
                    escaped.push(' ');
                }
            }
            _ => escaped.push(character),
        }
        previous = Some(character);
    }

    escaped
}

/// Finds the first line at or after `from` whose trimmed content equals
/// `marker`, returning the byte range of the line without its terminator.
fn find_marker(content: &str, marker: &str, from: usize) -> Option<(usize, usize)> {
    // Advance to the next char boundary so the scan never slices mid-character,
    // and clamp to the end of the content, so an offset that lands inside a
    // multi-byte scalar or past the end of the input cannot panic.
    let mut from = from.min(content.len());
    while from < content.len() && !content.is_char_boundary(from) {
        from += 1;
    }
    if from >= content.len() {
        // The offset reached the end of the content, so no line is left to scan.
        return None;
    }

    let mut line_start = line_start_of(content, from);
    loop {
        let (content_end, next_start) = line_bounds(content, line_start);
        let line = &content[line_start..content_end];
        if line.trim() == marker {
            // The range covers the line text only, without its line ending.
            return Some((line_start, line_start + line.len()));
        }
        if next_start <= line_start {
            break;
        }
        line_start = next_start;
    }

    None
}

/// The byte index where the line containing `end` starts, that is, just past
/// the last line terminator before `end`.
fn line_start_of(content: &str, end: usize) -> usize {
    content[..end]
        .char_indices()
        .rev()
        .find(|&(_, character)| character == '\n' || character == '\r')
        .map(|(index, character)| {
            let after = index + character.len_utf8();
            if character == '\r' && content.as_bytes().get(index + 1) == Some(&b'\n') {
                after + 1
            } else {
                after
            }
        })
        .unwrap_or(0)
}

/// The content end and the start of the next line of the line that begins at
/// `line_start`: the half-open range `[content_end, next_start)` covers the
/// line's terminator, and both equal `content.len()` when the line is
/// unterminated at end of input.
fn line_bounds(content: &str, line_start: usize) -> (usize, usize) {
    let Some((index, character)) = content[line_start..]
        .char_indices()
        .find(|&(_, character)| character == '\n' || character == '\r')
    else {
        return (content.len(), content.len());
    };
    let end = line_start + index;
    let next_start = if character == '\r' && content.as_bytes().get(end + 1) == Some(&b'\n') {
        end + 2
    } else {
        end + character.len_utf8()
    };
    (end, next_start)
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
            depends_on: Vec::new(),
            orphaned: false,
            feature_orphaned: false,
            supersedes: None,
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
    fn render_replaces_title_line_breaks_with_a_space() {
        let entry = feature("Add\nExport", Some("planned"), "add-export");

        assert_eq!(
            render(&[entry]),
            "- [Add Export](features/add-export.md) — planned\n"
        );
    }

    #[test]
    fn render_replaces_a_crlf_line_break_in_a_title_with_a_single_space() {
        let entry = feature("Add\r\nExport", Some("planned"), "add-export");

        assert_eq!(
            render(&[entry]),
            "- [Add Export](features/add-export.md) — planned\n"
        );
    }

    #[test]
    fn render_normalizes_an_unexpected_status_to_the_default() {
        let entry = feature("Export", Some("bogus"), "export");

        assert_eq!(
            render(&[entry]),
            "- [Export](features/export.md) — planned\n"
        );
    }

    #[test]
    fn render_normalizes_status_padding_and_line_breaks() {
        let entry = feature("Export", Some(" shipped\n"), "export");

        assert_eq!(
            render(&[entry]),
            "- [Export](features/export.md) — shipped\n"
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
    fn find_marker_does_not_compare_a_marker_against_its_own_line() {
        let prd = "<!-- sdlc:feature-index:start -->\n- stale\n<!-- sdlc:feature-index:end -->\n";
        let (_start, start_end) =
            find_marker(prd, FEATURE_INDEX_START, 0).expect("the start marker");
        // The duplicate scan starts past the marker's own line, so a valid
        // single-marker PRD never reports the marker as a duplicate of itself.
        assert!(find_marker(prd, FEATURE_INDEX_START, start_end + 1).is_none());
        let (_end, end_end) =
            find_marker(prd, FEATURE_INDEX_END, start_end).expect("the end marker");
        assert!(find_marker(prd, FEATURE_INDEX_END, end_end + 1).is_none());
    }

    #[test]
    fn rewrite_with_crlf_markers_and_a_duplicate_start_uses_the_first() {
        // A CRLF marker line puts the duplicate scans at `start_end + 1` and
        // `end_end + 1` between the CR and the LF of the terminator; the second
        // marker must still be found past the whole CRLF pair.
        let prd = "<!-- sdlc:feature-index:start -->\r\n- stale\r\n<!-- sdlc:feature-index:start -->\r\n- stale2\r\n<!-- sdlc:feature-index:end -->\r\n";
        let entry = feature("Export", Some("shipped"), "export");

        let RewriteOutcome::Updated(updated) = rewrite(prd, &[entry]) else {
            panic!("the rewrite should update");
        };

        assert!(updated.starts_with("<!-- sdlc:feature-index:start -->\r\n"));
        assert!(updated.contains("- [Export](features/export.md) — shipped\r\n"));
        assert!(updated.ends_with("<!-- sdlc:feature-index:end -->\r\n"));
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

    #[test]
    fn rewrite_uses_the_opening_marker_line_ending_for_a_mixed_ending_prd() {
        // The body mixes CRLF and LF, so the ending cannot be inferred from the
        // whole document: the generated lines follow the opening marker line.
        let prd = "# Title\n\nSome text.\r\n\r\n<!-- sdlc:feature-index:start -->\n- stale\n<!-- sdlc:feature-index:end -->\n";
        let entry = feature("Export", Some("shipped"), "export");

        let RewriteOutcome::Updated(updated) = rewrite(prd, &[entry]) else {
            panic!("the rewrite should update");
        };

        assert_eq!(
            updated,
            "# Title\n\nSome text.\r\n\r\n<!-- sdlc:feature-index:start -->\n- [Export](features/export.md) — shipped\n<!-- sdlc:feature-index:end -->\n".to_string()
        );
    }

    #[test]
    fn rewrite_keeps_the_opening_marker_line_ending_when_unterminated() {
        // An unterminated marker at end of input falls back to the preceding
        // line's ending instead of assuming LF, so a CRLF document never mixes
        // endings into the generated index.
        let crlf_prd = "# Title\r\n\r\nSome text.\r\n\r\n<!-- sdlc:feature-index:start -->";
        let crlf_marker = crlf_prd.find(FEATURE_INDEX_START).expect("the marker");
        assert_eq!(line_ending(crlf_prd, crlf_marker), "\r\n");

        let lf_prd = "# Title\n\nSome text.\n\n<!-- sdlc:feature-index:start -->";
        let lf_marker = lf_prd.find(FEATURE_INDEX_START).expect("the marker");
        assert_eq!(line_ending(lf_prd, lf_marker), "\n");
    }

    #[test]
    fn rewrite_reports_a_duplicate_end_marker_and_uses_the_first() {
        let prd = "<!-- sdlc:feature-index:start -->\n- stale\n<!-- sdlc:feature-index:end -->\n<!-- sdlc:feature-index:end -->\n";
        let entry = feature("Export", Some("shipped"), "export");

        let RewriteOutcome::Updated(updated) = rewrite(prd, &[entry]) else {
            panic!("the rewrite should update");
        };

        assert_eq!(
            updated,
            "<!-- sdlc:feature-index:start -->\n- [Export](features/export.md) — shipped\n<!-- sdlc:feature-index:end -->\n<!-- sdlc:feature-index:end -->\n"
        );
    }

    #[test]
    fn rewrite_handles_marker_lines_that_end_with_a_bare_cr() {
        // A marker line may end with a bare CR, which is its own terminator, so
        // the generated index lines follow it and the marker pair is still found.
        let prd = "<!-- sdlc:feature-index:start -->\r- stale\r<!-- sdlc:feature-index:end -->\r";
        let entry = feature("Export", Some("shipped"), "export");

        let RewriteOutcome::Updated(updated) = rewrite(prd, &[entry]) else {
            panic!("the rewrite should update");
        };

        assert_eq!(
            updated,
            "<!-- sdlc:feature-index:start -->\r- [Export](features/export.md) — shipped\r<!-- sdlc:feature-index:end -->\r"
        );
    }

    #[test]
    fn line_ending_treats_a_bare_cr_marker_line_terminator_as_a_bare_cr() {
        // The marker line ends with a bare CR, so the terminator must be read
        // from the first CR or LF of the line, not from an LF-only scan that
        // lands on a later line.
        let prd = "# Title\n\nSome text.\n<!-- sdlc:feature-index:start -->\r- stale\n<!-- sdlc:feature-index:end -->\n";
        let marker = prd.find(FEATURE_INDEX_START).expect("the marker");

        assert_eq!(line_ending(prd, marker), "\r");
    }

    #[test]
    fn line_ending_keeps_a_crlf_pair_that_follows_a_bare_cr() {
        // A CR that opens a CRLF pair is a terminator of its own even when the
        // preceding line already ended with a bare CR, so the pair must be kept
        // instead of being dropped as a duplicate continuation.
        let prd = "# Title\r\r\n<!-- sdlc:feature-index:start -->";
        let marker = prd.find(FEATURE_INDEX_START).expect("the marker");

        assert_eq!(line_ending(prd, marker), "\r\n");
    }

    #[test]
    fn line_start_of_steps_over_the_lf_of_a_crlf_pair() {
        let prd = "one\r\ntwo\r\nthree";
        assert_eq!(line_start_of(prd, 4), 5);
    }

    #[test]
    fn find_marker_skips_to_the_next_char_boundary() {
        let prd = "# Tïtle\n\n<!-- sdlc:feature-index:start -->\n<!-- sdlc:feature-index:end -->\n";
        // An offset that lands inside a multi-byte scalar must not panic; the
        // scan resumes at the next character boundary.
        let inside = prd.find('ï').expect("the multi-byte character");

        assert!(find_marker(prd, FEATURE_INDEX_START, inside + 1).is_some());
    }

    #[test]
    fn find_marker_returns_none_when_the_scan_offset_reaches_the_end_of_the_content() {
        let prd = "<!-- sdlc:feature-index:start -->";

        assert!(find_marker(prd, FEATURE_INDEX_START, prd.len() + 1).is_none());
    }
}
