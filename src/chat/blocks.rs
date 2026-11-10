//! Proposed file write and delete block parser (ticket 0040).
//!
//! The model proposes changes with plain-text blocks in its reply:
//!
//! ```text
//! === FILE: sdlc/features/export.md ===
//! The complete new content of the file.
//! === END FILE ===
//!
//! === DELETE: sdlc/epics/0001-foundation/tickets/0007-old-ticket.md ===
//! ```
//!
//! [`parse`] scans the full reply line by line and returns the accepted blocks
//! with the warnings for the malformed ones. A malformed block is skipped with a
//! warning instead of failing the conversation, and text outside blocks is
//! ignored because the chat already streamed it as ordinary reply text.

/// One proposed change extracted from a model reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// The complete new content of a document the model wants to write.
    Write { path: String, content: String },
    /// The document the model wants to delete.
    Delete { path: String },
}

/// The outcome of parsing one model reply: the accepted blocks and the warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// The accepted blocks, in the order the model proposed them.
    pub blocks: Vec<Block>,
    /// The warnings for the blocks that were skipped, in report order.
    pub warnings: Vec<String>,
}

/// Scans one full model reply and extracts its proposed write and delete blocks.
///
/// Returns the accepted blocks in the order the model proposed them and the
/// warnings for the blocks that were skipped; a reply without any block yields
/// an empty list and no warnings.
pub fn parse(content: &str) -> Parsed {
    let mut parsed = Parsed {
        blocks: Vec::new(),
        warnings: Vec::new(),
    };
    let mut open: Option<OpenWrite> = None;

    for line in content.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let trimmed = line.trim();
        let Some(marker) = marker(trimmed) else {
            // Text outside a block is ordinary chat; only lines inside a block
            // become content of the proposal.
            if let Some(block) = open.as_mut() {
                block.content.push_str(line);
                block.content.push('\n');
            }
            continue;
        };

        match marker {
            Marker::File { path } => {
                // A FILE marker while a block is open means the outer block never
                // saw its end marker, so the outer block is skipped and the new
                // block becomes the one being collected.
                if let Some(outer) = open.take() {
                    parsed.warnings.push(format!(
                        "nested FILE block before the end marker for {}: skipping the block",
                        outer.path
                    ));
                }
                open = Some(OpenWrite {
                    path,
                    content: String::new(),
                });
            }
            Marker::End => {
                if let Some(block) = open.take() {
                    accept_write(&mut parsed, block);
                }
            }
            Marker::Delete { path } => {
                accept_delete(&mut parsed, path);
            }
        }
    }

    if let Some(block) = open {
        parsed.warnings.push(format!(
            "unterminated FILE block for {}: missing end marker",
            block.path
        ));
    }

    parsed
}

/// A file write proposal that is being collected from the reply.
struct OpenWrite {
    /// The proposed target path, as written between the markers.
    path: String,
    /// The content of the proposal, with a final newline before the end marker.
    content: String,
}

/// A recognized marker line of a model reply.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Marker {
    /// `=== FILE: <path> ===`: the start of a proposed write.
    File { path: String },
    /// `=== END FILE ===`: the end of the proposed write being collected.
    End,
    /// `=== DELETE: <path> ===`: the proposed deletion.
    Delete { path: String },
}

/// Recognizes one trimmed marker line, or returns `None` for ordinary text.
///
/// The marker grammar is `=== FILE: <path> ===`, `=== END FILE ===` and
/// `=== DELETE: <path> ===`, with optional leading and trailing whitespace. A
/// path is whatever is between the colon and the closing `===`, trimmed, so
/// marker padding never leaks into the proposed path.
fn marker(line: &str) -> Option<Marker> {
    if let Some(path) = line
        .strip_prefix("=== FILE:")
        .and_then(|rest| rest.strip_suffix("==="))
        .map(str::trim)
    {
        return Some(Marker::File {
            path: path.to_string(),
        });
    }
    if let Some(path) = line
        .strip_prefix("=== DELETE:")
        .and_then(|rest| rest.strip_suffix("==="))
        .map(str::trim)
    {
        return Some(Marker::Delete {
            path: path.to_string(),
        });
    }
    if line == "=== END FILE ===" {
        return Some(Marker::End);
    }

    None
}

/// Turns a finished write proposal into a block, or skips it with a warning.
fn accept_write(parsed: &mut Parsed, block: OpenWrite) {
    if skip_invalid_path(parsed, "FILE", &block.path) {
        return;
    }

    push(
        parsed,
        Block::Write {
            path: block.path,
            content: block.content,
        },
    );
}

/// Turns a delete proposal into a block, or skips it with a warning.
fn accept_delete(parsed: &mut Parsed, path: String) {
    if skip_invalid_path(parsed, "DELETE", &path) {
        return;
    }

    push(parsed, Block::Delete { path });
}

/// Reports an invalid block path and returns `true` when the block must be skipped.
fn skip_invalid_path(parsed: &mut Parsed, kind: &str, path: &str) -> bool {
    if path.is_empty() {
        parsed
            .warnings
            .push(format!("the {kind} block has an empty path; skipping it"));
        return true;
    }
    false
}

/// Adds a block, replacing any earlier block for the same path and warning about it.
fn push(parsed: &mut Parsed, block: Block) {
    let path = path_of(&block);
    if let Some(index) = parsed
        .blocks
        .iter()
        .position(|block| path_of(block) == path)
    {
        parsed
            .warnings
            .push(format!("duplicate path {path}: keeping the last block"));
        parsed.blocks.remove(index);
    }
    parsed.blocks.push(block);
}

/// The path a block acts on, for duplicate detection.
fn path_of(block: &Block) -> &str {
    match block {
        Block::Write { path, .. } | Block::Delete { path } => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &str, content: &str) -> Block {
        Block::Write {
            path: path.to_string(),
            content: content.to_string(),
        }
    }

    fn delete(path: &str) -> Block {
        Block::Delete {
            path: path.to_string(),
        }
    }

    #[test]
    fn one_write_block_yields_the_exact_content() {
        let parsed = parse("=== FILE: sdlc/features/export.md ===\n# Export\n=== END FILE ===\n");

        assert_eq!(
            parsed.blocks,
            vec![write("sdlc/features/export.md", "# Export\n")]
        );
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn multiple_writes_a_delete_and_a_mixed_order_parse() {
        let parsed = parse(
            "=== DELETE: sdlc/epics/0001-foundation/tickets/0007-old-ticket.md ===\n\n\
             === FILE: sdlc/PRD.md ===\n# PRD\n=== END FILE ===\n\
             === FILE: sdlc/features/export.md ===\n# Export\n=== END FILE ===\n",
        );

        assert_eq!(parsed.blocks.len(), 3);
        assert_eq!(
            parsed.blocks[0],
            delete("sdlc/epics/0001-foundation/tickets/0007-old-ticket.md")
        );
        assert_eq!(parsed.blocks[1], write("sdlc/PRD.md", "# PRD\n"));
        assert_eq!(
            parsed.blocks[2],
            write("sdlc/features/export.md", "# Export\n")
        );
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn unterminated_block_is_skipped_with_a_warning() {
        let parsed =
            parse("I will update the document.\n=== FILE: sdlc/PRD.md ===\n# No end marker");

        assert!(parsed.blocks.is_empty());
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].contains("unterminated"));
    }

    #[test]
    fn empty_content_is_a_valid_write() {
        let parsed = parse("=== FILE: sdlc/features/export.md ===\n=== END FILE ===\n");

        assert_eq!(parsed.blocks, vec![write("sdlc/features/export.md", "")]);
    }

    #[test]
    fn duplicate_paths_produce_one_block_and_one_warning() {
        let parsed = parse(
            "=== FILE: sdlc/PRD.md ===\n# First\n=== END FILE ===\n\
             === FILE: sdlc/PRD.md ===\n# Second\n=== END FILE ===\n",
        );

        assert_eq!(parsed.blocks.len(), 1);
        assert_eq!(parsed.blocks[0], write("sdlc/PRD.md", "# Second\n"));
        assert_eq!(parsed.warnings.len(), 1);
    }

    #[test]
    fn marker_like_prose_without_an_end_marker_is_skipped() {
        let parsed =
            parse("Sure, I will look at it tomorrow.\n=== FILE: sdlc/PRD.md ===\n# Still no end");

        assert!(parsed.blocks.is_empty());
        assert_eq!(parsed.warnings.len(), 1);
    }

    #[test]
    fn crlf_input_yields_lf_content() {
        let parsed =
            parse("=== FILE: sdlc/features/export.md ===\r\n# Export\r\n=== END FILE ===\r\n");

        assert_eq!(
            parsed.blocks,
            vec![write("sdlc/features/export.md", "# Export\n")]
        );
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn end_marker_text_inside_a_code_fence_is_content() {
        let parsed = parse(
            "=== FILE: sdlc/features/export.md ===\n```rust\nlet x = \"=== END FILE ===\";\n```\n=== END FILE ===\n",
        );

        assert_eq!(
            parsed.blocks,
            vec![write(
                "sdlc/features/export.md",
                "```rust\nlet x = \"=== END FILE ===\";\n```\n"
            )]
        );
    }

    #[test]
    fn markers_inside_a_fenced_code_block_are_honored() {
        // The chosen behaviour: markers are recognized inside code fences too,
        // so a model that wraps its proposal in a fence still gets it parsed.
        let parsed =
            parse("```text\n=== FILE: sdlc/features/export.md ===\nhello\n=== END FILE ===\n```\n");

        assert_eq!(
            parsed.blocks,
            vec![write("sdlc/features/export.md", "hello\n")]
        );
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn marker_paths_are_trimmed_and_validation_happens_later() {
        let parsed = parse("=== FILE:   sdlc/features/export.md   ===\nhello\n=== END FILE ===\n");

        // The parser trims marker padding; DocPath validation (ticket 0041) is
        // what rejects paths that are not allowed documents.
        assert_eq!(
            parsed.blocks,
            vec![write("sdlc/features/export.md", "hello\n")]
        );
    }

    #[test]
    fn no_blocks_yields_no_blocks_and_no_warnings() {
        let parsed = parse("Just an ordinary chat answer about the plan.");

        assert!(parsed.blocks.is_empty());
        assert!(parsed.warnings.is_empty());
    }
}
