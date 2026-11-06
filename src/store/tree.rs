//! The document tree listing (ticket 0013).
//!
//! The tree answers the sidebar's first question in one read: which documents
//! exist, in what order, with which kind, title, status, epic and feature
//! binding. It walks the same allowed layout as every other store entry point,
//! so a path that is not a document never gets a tree entry, and one unreadable
//! file never fails the whole listing.

use std::path::Path;

use crate::store::frontmatter;
use crate::store::layout::{self, CoreDoc, DocKind};
use crate::store::path::DocPath;
use crate::store::{DocStore, StoreError};

/// One document in the project tree listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocEntry {
    /// The validated path of the document.
    pub path: DocPath,
    /// The kind of document this entry describes.
    pub kind: DocKind,
    /// The document title: the first ATX H1 outside front matter and fenced
    /// code blocks, or a humanized file name when the document has no H1.
    pub title: String,
    /// The document status, when the front matter carries a valid one.
    pub status: Option<String>,
    /// The epic directory of a ticket, derived from its path; `None` for every
    /// other kind.
    pub epic: Option<String>,
    /// The optional feature slug a ticket belongs to; `None` when absent.
    pub feature: Option<String>,
}

/// The document tree of one project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree {
    /// Whether the `sdlc/` layout directory exists at all.
    pub layout_exists: bool,
    /// The documents to list, in deterministic order.
    pub docs: Vec<DocEntry>,
}

/// The core documents in the fixed order the tree lists them in.
const CORE_DOCS: [(&str, DocKind); 4] = [
    ("sdlc/PRD.md", DocKind::Core(CoreDoc::Prd)),
    ("sdlc/architecture.md", DocKind::Core(CoreDoc::Architecture)),
    ("sdlc/conventions.md", DocKind::Core(CoreDoc::Conventions)),
    ("AGENTS.md", DocKind::Core(CoreDoc::Agents)),
];

impl DocStore {
    /// Walks the allowed layout and returns the document tree.
    ///
    /// The listing is best effort and read-only: core files appear only when
    /// they exist, the walk covers epic descriptions with their tickets,
    /// decisions and features, and files that fail validation or cannot be
    /// read are skipped with a warning instead of failing the whole listing.
    /// The result is deterministic, so repeated calls return equal trees.
    pub async fn tree(&self) -> Tree {
        let layout = self.root.join("sdlc");
        let layout_exists = tokio::fs::metadata(&layout)
            .await
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false);

        if !layout_exists {
            return Tree {
                layout_exists: false,
                docs: Vec::new(),
            };
        }

        let mut docs: Vec<DocEntry> = Vec::new();

        // Core documents in fixed order; only the ones that exist are listed.
        for (raw, kind) in CORE_DOCS {
            if let Some(entry) = self.read_at(raw, kind, None).await {
                docs.push(entry);
            }
        }

        // Epics by numeric directory prefix, each followed by its tickets by
        // numeric ticket prefix. A ticket whose epic directory has no
        // description is still listed, with its derived epic.
        let mut epic_names: Vec<(u32, String)> = Vec::new();
        let epics_dir = self.root.join("sdlc").join("epics");
        if let Ok(mut dir) = tokio::fs::read_dir(&epics_dir).await {
            while let Some(entry) = dir.next_entry().await.ok().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let description = format!("sdlc/epics/{name}/description.md");
                if names_kind(&description, DocKind::Epic) {
                    epic_names.push((numbered_prefix(&name), name));
                }
            }
        }
        epic_names.sort();

        for (_, name) in epic_names {
            let description = format!("sdlc/epics/{name}/description.md");
            let entry = self.read_at(&description, DocKind::Epic, None).await;
            if let Some(entry) = entry {
                docs.push(entry);
            } else {
                tracing::warn!(epic = %name, "epic description unavailable; listing its tickets anyway");
            }

            let tickets_dir = self
                .root
                .join("sdlc")
                .join("epics")
                .join(&name)
                .join("tickets");
            let mut tickets: Vec<(u32, DocEntry)> = Vec::new();
            if let Ok(mut dir) = tokio::fs::read_dir(&tickets_dir).await {
                while let Some(ticket) = dir.next_entry().await.ok().flatten() {
                    let ticket_name = ticket.file_name().to_string_lossy().into_owned();
                    let raw = format!("sdlc/epics/{name}/tickets/{ticket_name}");
                    if let Some(entry) = self
                        .read_at(&raw, DocKind::Ticket, Some(name.clone()))
                        .await
                    {
                        tickets.push((numbered_prefix(&ticket_name), entry));
                    }
                }
            }
            tickets.sort_by(|a, b| {
                a.0.cmp(&b.0)
                    .then_with(|| a.1.path.as_path().cmp(b.1.path.as_path()))
            });
            docs.extend(tickets.into_iter().map(|(_, entry)| entry));
        }

        // Decisions by number, then features by title.
        let mut decisions: Vec<(u32, DocEntry)> = Vec::new();
        let decisions_dir = self.root.join("sdlc").join("decisions");
        if let Ok(mut dir) = tokio::fs::read_dir(&decisions_dir).await {
            while let Some(entry) = dir.next_entry().await.ok().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let raw = format!("sdlc/decisions/{name}");
                if let Some(decision) = self.read_at(&raw, DocKind::Decision, None).await {
                    decisions.push((numbered_prefix(&name), decision));
                }
            }
        }
        decisions.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.path.as_path().cmp(b.1.path.as_path()))
        });
        docs.extend(decisions.into_iter().map(|(_, entry)| entry));

        let mut features: Vec<DocEntry> = Vec::new();
        let features_dir = self.root.join("sdlc").join("features");
        if let Ok(mut dir) = tokio::fs::read_dir(&features_dir).await {
            while let Some(entry) = dir.next_entry().await.ok().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let raw = format!("sdlc/features/{name}");
                if let Some(entry) = self.read_at(&raw, DocKind::Feature, None).await {
                    features.push(entry);
                }
            }
        }
        features.sort_by(|a, b| a.title.cmp(&b.title));
        docs.extend(features);

        Tree {
            layout_exists: true,
            docs,
        }
    }

    /// Reads one document and turns it into a tree entry.
    ///
    /// Returns `None` when the document cannot be read, so one odd file never
    /// fails the whole listing.
    async fn read_entry(
        &self,
        path: &DocPath,
        kind: DocKind,
        epic: Option<String>,
    ) -> Option<DocEntry> {
        let file = path.as_path().display().to_string();
        let file_name = path
            .as_path()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let document = match self.read(path).await {
            Ok(document) => document,
            Err(err) => {
                if matches!(err, StoreError::NotFound(_)) {
                    tracing::debug!(path = %file, "skipping a missing document in the tree");
                } else {
                    tracing::warn!(error = %err, path = %file, "skipping an unreadable document in the tree");
                }
                return None;
            }
        };
        let (status, feature) = metadata_of(kind, &document.content, &file);

        Some(DocEntry {
            path: path.clone(),
            kind,
            title: title_of(&document.content, &file_name),
            status,
            epic,
            feature,
        })
    }

    /// Reads one document from a raw layout path, matching the expected kind,
    /// and turns it into a tree entry; `None` when the path is not a valid
    /// document of that kind or the file cannot be read.
    async fn read_at(&self, raw: &str, kind: DocKind, epic: Option<String>) -> Option<DocEntry> {
        if let Ok(path) = DocPath::parse(raw)
            && layout::kind(path.as_path()) == Some(kind)
        {
            return self.read_entry(&path, kind, epic).await;
        }

        None
    }
}

/// The feature entries of one project, in store order, for the PRD feature index.
///
/// This is the single feature traversal behind the PRD feature index refresh, so
/// the rendered index cannot drift from the feature part of [`DocStore::tree`]: it
/// applies the same filtering, reading, title/status extraction and sort order.
/// Like the tree listing it is best effort, and the entries it skips are reported
/// as warnings, so a refresh can surface an incomplete index instead of silently
/// rendering one from a partial list.
pub(crate) fn feature_entries(root: &Path) -> (Vec<DocEntry>, Vec<String>) {
    let features_dir = root.join("sdlc").join("features");
    let Ok(directory) = std::fs::read_dir(&features_dir) else {
        return (Vec::new(), Vec::new());
    };

    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    for entry in directory.filter_map(|entry| entry.ok()) {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let raw = format!("sdlc/features/{file_name}");
        let Ok(path) = DocPath::parse(&raw) else {
            warnings.push(format!(
                "the PRD feature index was not refreshed: skipping {raw}: not a valid feature path"
            ));
            continue;
        };
        if layout::kind(path.as_path()) != Some(DocKind::Feature) {
            warnings.push(format!(
                "the PRD feature index was not refreshed: skipping {raw}: not a feature"
            ));
            continue;
        }

        // Canonicalize before reading, so a symlink that escapes the project
        // root is rejected before its content can reach the rendered index, the
        // same containment rule the tree read path applies.
        let file_path = root.join("sdlc").join("features").join(&file_name);
        let resolved = match std::fs::canonicalize(&file_path) {
            Ok(resolved) => resolved,
            Err(error) => {
                warnings.push(format!(
                    "the PRD feature index was not refreshed: skipping {raw}: {error}"
                ));
                continue;
            }
        };
        let resolved = match crate::store::ensure_inside_root(root, &path, resolved) {
            Ok(resolved) => resolved,
            Err(_) => {
                warnings.push(format!(
                    "the PRD feature index was not refreshed: skipping {file_name}: resolves outside the project root"
                ));
                continue;
            }
        };

        let document = match crate::store::read_document(&resolved, &path) {
            Ok(document) => document,
            Err(error) => {
                warnings.push(format!(
                    "the PRD feature index was not refreshed: skipping feature {file_name}: {error}"
                ));
                continue;
            }
        };

        entries.push(DocEntry {
            path,
            kind: DocKind::Feature,
            title: title_of(&document.content, &file_name),
            status: frontmatter::parse_feature(&document.content, &file_name)
                .status
                .map(|status| status.as_str().to_string()),
            epic: None,
            feature: None,
        });
    }

    entries.sort_by(|left, right| left.title.cmp(&right.title));
    (entries, warnings)
}

/// The status and optional feature binding of a document, parsed from a
/// single front matter pass. Only tickets carry a feature binding.
fn metadata_of(kind: DocKind, content: &str, file: &str) -> (Option<String>, Option<String>) {
    match kind {
        DocKind::Core(_) => (None, None),
        DocKind::Epic => {
            let meta = frontmatter::parse_epic(content, file);
            (meta.status.map(|status| status.as_str().to_owned()), None)
        }
        DocKind::Feature => {
            let meta = frontmatter::parse_feature(content, file);
            (meta.status.map(|status| status.as_str().to_owned()), None)
        }
        DocKind::Decision => {
            let meta = frontmatter::parse_decision(content, file);
            (meta.status.map(|status| status.as_str().to_owned()), None)
        }
        DocKind::Ticket => {
            let meta = frontmatter::parse_ticket(content, file);
            (
                meta.status.map(|status| status.as_str().to_owned()),
                meta.feature
                    .map(|feature| feature.strip_suffix(".md").unwrap_or(&feature).to_owned()),
            )
        }
    }
}

/// Reports whether a raw layout path names a document of the expected kind.
fn names_kind(raw: &str, kind: DocKind) -> bool {
    DocPath::parse(raw).is_ok_and(|path| layout::kind(path.as_path()) == Some(kind))
}

/// Extracts the document title: the first ATX H1 outside front matter and
/// fenced code blocks, falling back to the humanized file name.
pub(crate) fn title_of(content: &str, file: &str) -> String {
    let (_, body) = frontmatter::split(content);
    let mut in_fence = false;
    for line in body.lines() {
        if is_fence(line) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(title) = atx_h1(line) {
            return title;
        }
    }
    humanize(file)
}

/// Reports whether a line opens or closes a fenced code block.
fn is_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

/// The text of the first ATX H1 heading, without its `#` markers.
///
/// Only an H1 counts, so `##` deeper headings are not titles, and the closing
/// sequence of trailing `#`s is not part of the text.
fn atx_h1(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix('#')?;
    if !rest.is_empty() && !rest.starts_with(' ') {
        // A `#` directly followed by another `#` is a deeper heading, not an H1.
        return None;
    }
    let text = rest.strip_prefix(' ').unwrap_or("");
    let trimmed = text.trim_end_matches('#').trim_end();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Humanizes a file name for display, dropping the numeric prefix:
/// `0007-add-export` becomes `Add export`.
fn humanize(file: &str) -> String {
    let stem = file.strip_suffix(".md").unwrap_or(file);
    let stem = match stem.split_once('-') {
        Some((prefix, rest))
            if prefix.len() == 4 && prefix.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            rest
        }
        _ => stem,
    };

    let spaced = stem.replace('-', " ");
    let mut chars = spaced.trim().chars();
    let mut title = String::with_capacity(spaced.len() + 1);
    if let Some(first) = chars.next() {
        title.extend(first.to_uppercase());
    }
    for character in chars {
        title.push(character);
    }

    title
}

/// The numeric prefix of a numbered document name such as `0007-add-export`,
/// or `0` when the name is not numbered.
fn numbered_prefix(name: &str) -> u32 {
    layout::numbered_number(name).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::layout::{CoreDoc, DocKind};
    use std::fs;

    async fn store_in_temp() -> (tempfile::TempDir, DocStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = DocStore::new(dir.path().to_path_buf()).expect("store");

        (dir, store)
    }

    fn write(dir: &tempfile::TempDir, relative: &str, content: &str) {
        let path = dir.path().join(relative);
        fs::create_dir_all(path.parent().expect("the parent directory")).expect("create parent");
        fs::write(path, content).expect("write the file");
    }

    #[tokio::test]
    async fn empty_project_returns_no_layout_and_no_documents() {
        let (_dir, store) = store_in_temp().await;
        let tree = store.tree().await;

        assert!(!tree.layout_exists);
        assert!(tree.docs.is_empty());
    }

    #[tokio::test]
    async fn layout_exists_is_true_for_a_partial_layout_and_false_for_none() {
        let (dir, store) = store_in_temp().await;
        assert!(!store.tree().await.layout_exists);

        write(&dir, "sdlc/PRD.md", "# PRD\n");

        let tree = store.tree().await;
        assert!(tree.layout_exists);
        assert_eq!(tree.docs.len(), 1);
        assert_eq!(tree.docs[0].kind, DocKind::Core(CoreDoc::Prd));
    }

    #[tokio::test]
    async fn tree_lists_a_fixture_project_in_a_deterministic_order() {
        let (dir, store) = store_in_temp().await;
        write(&dir, "sdlc/PRD.md", "# 0002: Document store\n");
        write(&dir, "sdlc/architecture.md", "# 0002: Architecture\n");
        write(&dir, "sdlc/conventions.md", "# 0002: Conventions\n");
        write(&dir, "AGENTS.md", "# sdlc\n");
        write(
            &dir,
            "sdlc/epics/0001-foundation/description.md",
            "---\nstatus: planned\n---\n\n# 0001: Foundation and server shell\n",
        );
        write(
            &dir,
            "sdlc/epics/0001-foundation/tickets/0001-project.md",
            "---\nstatus: todo\nfeature: export\n---\n\n# 0001: Project skeleton\n",
        );
        write(
            &dir,
            "sdlc/epics/0001-foundation/tickets/0002-add-export.md",
            "---\nstatus: in-progress\nfeature: export\n---\n\n# 0002: Add CSV export endpoint\n",
        );
        write(
            &dir,
            "sdlc/decisions/0003-use-axum.md",
            "---\nstatus: proposed\n---\n\n# 0003: Use axum\n",
        );
        write(
            &dir,
            "sdlc/features/export.md",
            "---\nstatus: shipped\n---\n\n# Export\n",
        );
        write(
            &dir,
            "sdlc/features/v2-api.md",
            "---\nstatus: planned\n---\n\n# V2 API\n",
        );

        let tree = store.tree().await;
        let tree_again = store.tree().await;

        assert_eq!(tree, tree_again);
        assert!(tree.layout_exists);
        assert_eq!(tree.docs.len(), 10);

        assert_eq!(tree.docs[0].kind, DocKind::Core(CoreDoc::Prd));
        assert_eq!(tree.docs[1].kind, DocKind::Core(CoreDoc::Architecture));
        assert_eq!(tree.docs[2].kind, DocKind::Core(CoreDoc::Conventions));
        assert_eq!(tree.docs[3].kind, DocKind::Core(CoreDoc::Agents));
        assert_eq!(tree.docs[4].kind, DocKind::Epic);
        assert_eq!(tree.docs[4].status.as_deref(), Some("planned"));

        let first_ticket = &tree.docs[5];
        assert_eq!(first_ticket.kind, DocKind::Ticket);
        assert_eq!(first_ticket.epic.as_deref(), Some("0001-foundation"));
        assert_eq!(first_ticket.feature.as_deref(), Some("export"));
        assert_eq!(first_ticket.status.as_deref(), Some("todo"));

        let second_ticket = &tree.docs[6];
        assert_eq!(second_ticket.kind, DocKind::Ticket);
        assert_eq!(second_ticket.status.as_deref(), Some("in-progress"));

        assert_eq!(tree.docs[7].kind, DocKind::Decision);
        assert_eq!(tree.docs[7].status.as_deref(), Some("proposed"));

        assert_eq!(tree.docs[8].kind, DocKind::Feature);
        assert_eq!(tree.docs[8].status.as_deref(), Some("shipped"));
        assert_eq!(tree.docs[8].title, "Export");

        assert_eq!(tree.docs[9].kind, DocKind::Feature);
        assert_eq!(tree.docs[9].status.as_deref(), Some("planned"));
    }

    #[tokio::test]
    async fn tree_title_falls_back_to_a_humanized_file_name() {
        let (dir, store) = store_in_temp().await;
        write(
            &dir,
            "sdlc/decisions/0007-add-export.md",
            "No heading here.\n",
        );

        let tree = store.tree().await;

        assert_eq!(tree.docs.len(), 1);
        assert_eq!(tree.docs[0].kind, DocKind::Decision);
        assert_eq!(tree.docs[0].title, "Add export");
    }

    #[tokio::test]
    async fn tree_title_takes_the_first_atx_h1_and_skips_fenced_code_blocks() {
        let (dir, store) = store_in_temp().await;
        write(
            &dir,
            "sdlc/features/export.md",
            "# First\n\n## Not it\n\n# Second\n",
        );

        let tree = store.tree().await;

        assert_eq!(tree.docs.len(), 1);
        assert_eq!(tree.docs[0].title, "First");

        let (dir, store) = store_in_temp().await;
        write(
            &dir,
            "sdlc/features/export.md",
            "```\n# not a heading\n```\n\n# Real Title\n",
        );

        let tree = store.tree().await;

        assert_eq!(tree.docs.len(), 1);
        assert_eq!(tree.docs[0].title, "Real Title");
    }

    #[tokio::test]
    async fn tree_skips_invalidly_named_and_unreadable_documents() {
        let (dir, store) = store_in_temp().await;
        write(&dir, "sdlc/features/export.md", "# Export\n");
        write(&dir, "sdlc/features/Bad.md", "# Bad\n");
        let bad_decision = dir.path().join("sdlc/decisions/0009-weird.md");
        fs::create_dir_all(bad_decision.parent().expect("the parent directory"))
            .expect("create parent");
        fs::write(&bad_decision, [0xff, 0xfe, b'n', b'o']).expect("write invalid utf8");

        let tree = store.tree().await;

        assert_eq!(tree.docs.len(), 1);
        assert_eq!(tree.docs[0].kind, DocKind::Feature);
        assert_eq!(tree.docs[0].title, "Export");
    }

    #[tokio::test]
    async fn tree_lists_a_ticket_whose_epic_has_no_description() {
        let (dir, store) = store_in_temp().await;
        write(
            &dir,
            "sdlc/epics/0002-ghost/tickets/0001-orphan.md",
            "---\nstatus: todo\n---\n\n# 0001: Orphan ticket\n",
        );

        let tree = store.tree().await;

        assert_eq!(tree.docs.len(), 1);
        let entry = &tree.docs[0];
        assert_eq!(entry.kind, DocKind::Ticket);
        assert_eq!(entry.epic.as_deref(), Some("0002-ghost"));
        assert_eq!(entry.status.as_deref(), Some("todo"));
    }

    #[tokio::test]
    async fn tree_reports_the_feature_binding_of_a_ticket() {
        let (dir, store) = store_in_temp().await;
        write(
            &dir,
            "sdlc/epics/0001-foundation/tickets/0001-project.md",
            "---\nstatus: todo\nfeature: export.md\n---\n\n# 0001: Project skeleton\n",
        );

        let tree = store.tree().await;

        assert_eq!(tree.docs.len(), 1);
        let entry = &tree.docs[0];
        assert_eq!(entry.kind, DocKind::Ticket);
        assert_eq!(entry.status.as_deref(), Some("todo"));
        assert_eq!(entry.feature.as_deref(), Some("export"));
    }

    #[test]
    fn feature_entries_collects_features_in_store_order_and_skips_invalid_ones() {
        let dir = tempfile::tempdir().expect("tempdir");
        let features = dir.path().join("sdlc").join("features");
        std::fs::create_dir_all(&features).expect("features dir");
        std::fs::write(
            features.join("export.md"),
            "---\nstatus: shipped\n---\n\n# Export\n",
        )
        .expect("write the feature");
        std::fs::write(features.join("Bad.md"), "# Bad\n").expect("write the bad feature");

        let (entries, warnings) = feature_entries(dir.path());

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Export");
        assert_eq!(entries[0].status.as_deref(), Some("shipped"));
        assert_eq!(
            warnings.len(),
            1,
            "the invalid feature should be skipped with a warning"
        );
    }

    #[cfg(unix)]
    #[test]
    fn feature_entries_rejects_a_symlink_that_escapes_the_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let features = dir.path().join("sdlc").join("features");
        std::fs::create_dir_all(&features).expect("features dir");
        let outside = dir
            .path()
            .parent()
            .expect("the tempdir parent")
            .join("sdlc-feature-escape.md");
        std::fs::write(&outside, "# Escaped\n").expect("write the outside file");
        std::os::unix::fs::symlink(&outside, features.join("evil.md")).expect("create the symlink");

        let (entries, warnings) = feature_entries(dir.path());

        assert!(entries.is_empty());
        assert_eq!(
            warnings.len(),
            1,
            "the escaping symlink should be skipped with a warning"
        );
    }

    #[tokio::test]
    async fn tree_reports_a_null_status_for_an_invalid_ticket_status() {
        let (dir, store) = store_in_temp().await;
        write(
            &dir,
            "sdlc/epics/0001-foundation/tickets/0001-project.md",
            "---\nstatus: bogus\n---\n\n# 0001: Project skeleton\n",
        );

        let tree = store.tree().await;

        assert_eq!(tree.docs.len(), 1);
        let entry = &tree.docs[0];
        assert_eq!(entry.kind, DocKind::Ticket);
        assert_eq!(entry.status, None);
    }
}
