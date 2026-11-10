//! Proposed file writes and deletes turned into reviewable proposals (ticket 0041).
//!
//! The block parser turns a model reply into [`Block`]s; this module turns the
//! accepted blocks into a [`Proposal`] that carries the classified change, the
//! content hash the proposal was based on and the unified diff a reviewer reads
//! before accepting.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use similar::TextDiff;
use thiserror::Error;

use crate::chat::Block;
use crate::store::{DocPath, DocStore, StoreError};
use uuid::Uuid;

/// How many context lines the unified diff keeps around each change.
const DIFF_CONTEXT_LINES: usize = 3;

/// The refusal reasons of a proposal build.
#[derive(Debug, Error)]
pub enum ProposalError {
    /// The proposed path is not an allowed document path.
    #[error("the proposed path is not an allowed document: {0}")]
    InvalidPath(String),

    /// The proposal repeats a path it already carries a change for.
    #[error("duplicate proposed path: {0}")]
    DuplicatePath(String),

    /// The proposal deletes a document that does not exist.
    #[error("the proposed document is missing: {0}")]
    MissingTarget(String),

    /// The proposal carries no change at all.
    #[error("the proposal has no changes")]
    EmptyProposal,

    /// A document store operation failed while the proposal was built.
    #[error("the document store failed: {0}")]
    Store(#[from] StoreError),

    /// The diff task failed while the proposal was built.
    #[error("the proposal diff task failed: {0}")]
    DiffTask(#[from] tokio::task::JoinError),
}

/// The kind of change a proposal asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// The document did not exist yet, so the proposal creates it.
    Create,
    /// The document existed and the proposal replaces its content.
    Modify,
    /// The document existed and the proposal deletes it.
    Delete,
}

/// One validated change of a proposal: what to do, to which content, from which
/// hash and as which unified diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The validated path of the document the change acts on.
    pub path: DocPath,
    /// The kind of change: create, modify or delete.
    pub action: Action,
    /// The proposed content for a create or modify; `None` for a delete.
    pub new_content: Option<String>,
    /// The hash of the content the proposal was based on; `None` for a create,
    /// which is only valid while the file is still missing.
    pub base_hash: Option<String>,
    /// The unified diff of the change, with `--- a/<path>` and `+++ b/<path>`
    /// headers and three lines of context.
    pub diff: String,
}

/// A reviewable proposal: the identifier a client uses to accept or reject it and
/// the ordered changes it asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// The unique identifier of the proposal.
    pub id: String,
    /// The changes of the proposal, in the order the model proposed them.
    pub changes: Vec<Change>,
}

/// Builds and holds the in-memory proposals of one server process.
///
/// The map keeps every built proposal for the lifetime of the process: proposals
/// are only removed by the accept/reject lifecycle tickets that follow this one,
/// and they die with the process because nothing is persisted.
#[derive(Debug)]
pub struct ProposalService {
    store: Arc<DocStore>,
    proposals: Mutex<std::collections::HashMap<String, Proposal>>,
}

impl ProposalService {
    /// Creates a service for one store; proposals live only in memory and die
    /// with the process.
    pub fn new(store: Arc<DocStore>) -> Self {
        Self {
            store,
            proposals: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Turns parsed chat blocks into a validated proposal.
    ///
    /// Every block path is parsed into a [`DocPath`] first and one bad path
    /// rejects the whole proposal; duplicate paths are refused as well. Each
    /// accepted write is classified against the content the store holds at this
    /// moment (`Modify` with the base hash, or `Create` while the file is still
    /// missing), and each delete must name an existing document. The unified
    /// diff of every change is computed from the content read here, so a change
    /// is never diffed against a second read of the same file.
    ///
    /// The proposal is stored in memory and returned; it only becomes real when
    /// a later ticket accepts it.
    pub async fn create(&self, blocks: &[Block]) -> Result<Proposal, ProposalError> {
        let mut changes: Vec<Change> = Vec::new();
        let mut seen: HashSet<DocPath> = HashSet::new();

        for block in blocks {
            let raw_path = block_path(block);
            let path = DocPath::parse(raw_path)
                .map_err(|_| ProposalError::InvalidPath(raw_path.to_string()))?;
            if !seen.insert(path.clone()) {
                return Err(ProposalError::DuplicatePath(raw_path.to_string()));
            }

            let (action, new_content, base_hash, base_content) = match block {
                Block::Write { content, .. } => {
                    let document = self.store.read(&path).await;
                    match document {
                        Ok(document) => (
                            Action::Modify,
                            Some(content.clone()),
                            Some(document.hash),
                            document.content,
                        ),
                        // A missing file is a create; the base hash of a create is
                        // `None` because there is no content the proposal replaced.
                        Err(StoreError::NotFound(_)) => {
                            (Action::Create, Some(content.clone()), None, String::new())
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                Block::Delete { .. } => {
                    let document = self.store.read(&path).await;
                    match document {
                        Ok(document) => {
                            (Action::Delete, None, Some(document.hash), document.content)
                        }
                        Err(StoreError::NotFound(_)) => {
                            return Err(ProposalError::MissingTarget(raw_path.to_string()));
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
            };

            let new_for_diff = new_content.clone().unwrap_or_default();
            let path_for_diff = path.clone();
            let base_for_diff = base_content;
            let diff = tokio::task::spawn_blocking(move || {
                unified_diff(&path_for_diff, &base_for_diff, &new_for_diff)
            })
            .await?;

            changes.push(Change {
                path,
                action,
                new_content,
                base_hash,
                diff,
            });
        }

        if changes.is_empty() {
            return Err(ProposalError::EmptyProposal);
        }

        let proposal = Proposal {
            id: Uuid::new_v4().to_string(),
            changes,
        };
        self.proposals
            .lock()
            .expect("the proposal map lock should not be poisoned")
            .insert(proposal.id.clone(), proposal.clone());

        Ok(proposal)
    }

    /// Returns the stored proposal, or `None` when the id is unknown.
    pub fn get(&self, id: &str) -> Option<Proposal> {
        self.proposals
            .lock()
            .expect("the proposal map lock should not be poisoned")
            .get(id)
            .cloned()
    }
}

/// The path a block acts on, as written between the markers.
fn block_path(block: &Block) -> &str {
    match block {
        Block::Write { path, .. } | Block::Delete { path } => path,
    }
}

/// Computes the unified diff of `base` and `new` for one document.
///
/// The diff keeps three lines of context around every change and carries the
/// `--- a/<path>` / `+++ b/<path>` headers of the unified diff format; a create
/// diffs from the empty content and a delete diffs towards the empty content, so
/// both are plain additions or removals of the document lines.
fn unified_diff(path: &DocPath, base: &str, new: &str) -> String {
    let text_diff = TextDiff::from_lines(base, new);
    let mut diff = text_diff.unified_diff();
    diff.context_radius(DIFF_CONTEXT_LINES);
    diff.header(
        &format!("a/{}", path.as_path().display()),
        &format!("b/{}", path.as_path().display()),
    );
    diff.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Block;
    use crate::store::{DocStore, content_hash};
    use std::sync::Arc;
    use tempfile::TempDir;

    async fn service_in_temp() -> (TempDir, ProposalService) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(DocStore::new(dir.path().to_path_buf()).expect("store"));
        let service = ProposalService::new(store);

        (dir, service)
    }

    fn write_block(path: &str, content: &str) -> Block {
        Block::Write {
            path: path.to_string(),
            content: content.to_string(),
        }
    }

    fn delete_block(path: &str) -> Block {
        Block::Delete {
            path: path.to_string(),
        }
    }

    fn feature_file(dir: &TempDir, name: &str, content: &str) -> std::path::PathBuf {
        let path = dir.path().join("sdlc/features").join(name);
        std::fs::create_dir_all(path.parent().expect("feature dir")).expect("feature dir");
        std::fs::write(&path, content).expect("write feature");
        path
    }

    #[tokio::test]
    async fn empty_block_list_is_rejected() {
        let (_dir, service) = service_in_temp().await;

        let error = service.create(&[]).await.expect_err("empty proposal");

        assert!(matches!(error, ProposalError::EmptyProposal));
    }

    #[tokio::test]
    async fn write_to_a_missing_file_is_a_create() {
        let (_dir, service) = service_in_temp().await;
        let blocks = [write_block("sdlc/features/export.md", "# Export\n")];

        let proposal = service.create(&blocks).await.expect("the proposal");

        assert_eq!(proposal.changes.len(), 1);
        let change = &proposal.changes[0];
        assert_eq!(change.action, Action::Create);
        assert_eq!(change.base_hash, None);
        assert_eq!(change.new_content.as_deref(), Some("# Export\n"));
        assert!(service.get(&proposal.id).is_some());
    }

    #[tokio::test]
    async fn write_to_an_existing_file_is_a_modify_with_the_base_hash() {
        let (dir, service) = service_in_temp().await;
        feature_file(&dir, "export.md", "# Export\n");
        let blocks = [write_block("sdlc/features/export.md", "# Exported\n")];

        let proposal = service.create(&blocks).await.expect("the proposal");

        let change = &proposal.changes[0];
        assert_eq!(change.action, Action::Modify);
        assert_eq!(
            change.base_hash.as_deref(),
            Some(content_hash(b"# Export\n").as_str())
        );
        assert_eq!(change.new_content.as_deref(), Some("# Exported\n"));
    }

    #[tokio::test]
    async fn delete_requires_an_existing_file() {
        let (_dir, service) = service_in_temp().await;

        let error = service
            .create(&[delete_block("sdlc/features/nope.md")])
            .await
            .expect_err("deleting a missing file");

        assert!(matches!(error, ProposalError::MissingTarget(_)));
    }

    #[tokio::test]
    async fn delete_of_an_existing_file_is_a_delete_with_the_base_hash() {
        let (dir, service) = service_in_temp().await;
        feature_file(&dir, "export.md", "# Export\n");
        let blocks = [delete_block("sdlc/features/export.md")];

        let proposal = service.create(&blocks).await.expect("the proposal");

        let change = &proposal.changes[0];
        assert_eq!(change.action, Action::Delete);
        assert_eq!(
            change.base_hash.as_deref(),
            Some(content_hash(b"# Export\n").as_str())
        );
        assert_eq!(change.new_content, None);
    }

    #[tokio::test]
    async fn one_invalid_path_rejects_the_whole_proposal() {
        let (_dir, service) = service_in_temp().await;
        let blocks = [
            write_block("sdlc/PRD.md", "# PRD\n"),
            write_block("../AGENTS.md", "# Escape\n"),
        ];

        let error = service.create(&blocks).await.expect_err("invalid proposal");

        assert!(matches!(error, ProposalError::InvalidPath(_)));
    }

    #[tokio::test]
    async fn absolute_backslash_and_traversal_paths_are_invalid() {
        let (_dir, service) = service_in_temp().await;

        for raw in [
            "/etc/passwd",
            "sdlc\\PRD.md",
            "sdlc/PRD.md/../../AGENTS.md",
            "",
        ] {
            let error = service
                .create(&[write_block(raw, "# No\n")])
                .await
                .expect_err("invalid path");

            assert!(
                matches!(error, ProposalError::InvalidPath(_)),
                "the path {raw:?} should be rejected"
            );
        }
    }

    #[tokio::test]
    async fn duplicate_paths_are_rejected() {
        let (_dir, service) = service_in_temp().await;
        let blocks = [
            write_block("sdlc/PRD.md", "# First\n"),
            write_block("sdlc/PRD.md", "# Second\n"),
        ];

        let error = service
            .create(&blocks)
            .await
            .expect_err("duplicate proposal");

        assert!(matches!(error, ProposalError::DuplicatePath(_)));
    }

    #[tokio::test]
    async fn same_content_yields_an_empty_diff_but_a_valid_proposal() {
        let (dir, service) = service_in_temp().await;
        feature_file(&dir, "export.md", "# Export\n");
        let blocks = [write_block("sdlc/features/export.md", "# Export\n")];

        let proposal = service.create(&blocks).await.expect("the proposal");

        let change = &proposal.changes[0];
        assert_eq!(change.action, Action::Modify);
        assert_eq!(change.diff, "");
        assert_eq!(
            change.base_hash.as_deref(),
            Some(content_hash(b"# Export\n").as_str())
        );
    }

    #[tokio::test]
    async fn proposal_ids_are_unique() {
        let (_dir, service) = service_in_temp().await;
        let first = service
            .create(&[write_block("sdlc/PRD.md", "# First\n")])
            .await
            .expect("first proposal");
        let second = service
            .create(&[write_block("sdlc/PRD.md", "# Second\n")])
            .await
            .expect("second proposal");

        assert_ne!(first.id, second.id);
        assert_eq!(service.get(&first.id).expect("first stored").id, first.id);
    }

    #[tokio::test]
    async fn diff_of_a_small_change_shows_context_and_counts() {
        let (dir, service) = service_in_temp().await;
        feature_file(&dir, "export.md", "# Old\n\nBody line\n");
        let blocks = [write_block(
            "sdlc/features/export.md",
            "# New\n\nBody line\n",
        )];

        let proposal = service.create(&blocks).await.expect("the proposal");
        let change = &proposal.changes[0];

        assert_eq!(change.action, Action::Modify);
        assert_eq!(
            change.base_hash.as_deref(),
            Some(content_hash(b"# Old\n\nBody line\n").as_str())
        );
        assert!(change.diff.contains("--- a/sdlc/features/export.md"));
        assert!(change.diff.contains("+++ b/sdlc/features/export.md"));
        assert!(change.diff.contains("-# Old"));
        assert!(change.diff.contains("+# New"));
        assert!(change.diff.contains(" Body line"));
        assert_eq!(
            change
                .diff
                .lines()
                .filter(|line| line.starts_with('+') && !line.starts_with("+++ "))
                .count(),
            1
        );
        assert_eq!(
            change
                .diff
                .lines()
                .filter(|line| line.starts_with('-') && !line.starts_with("--- "))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn create_diff_adds_lines_and_delete_diff_removes_them() {
        let (_dir, service) = service_in_temp().await;
        let created = service
            .create(&[write_block(
                "sdlc/features/export.md",
                "# Export\n\nHello\n",
            )])
            .await
            .expect("create proposal");
        let created_change = &created.changes[0];
        assert_eq!(created_change.action, Action::Create);
        assert_eq!(created_change.base_hash, None);
        assert!(
            created_change
                .diff
                .starts_with("--- a/sdlc/features/export.md\n")
        );
        assert!(
            created_change
                .diff
                .contains("+++ b/sdlc/features/export.md")
        );
        assert!(created_change.diff.contains("+# Export"));
        assert!(created_change.diff.contains("+Hello"));

        let (dir, service) = service_in_temp().await;
        feature_file(&dir, "export.md", "# Export\n");
        let deleted = service
            .create(&[delete_block("sdlc/features/export.md")])
            .await
            .expect("delete proposal");

        let delete_change = &deleted.changes[0];
        assert_eq!(delete_change.action, Action::Delete);
        assert!(delete_change.diff.contains("-# Export"));
        assert!(!delete_change.diff.contains("+# Export"));
    }

    #[tokio::test]
    async fn large_file_diff_is_computed_for_the_proposed_content() {
        let (dir, service) = service_in_temp().await;
        let base = (0..500)
            .map(|index| format!("line {index}\n"))
            .collect::<Vec<_>>()
            .join("");
        feature_file(&dir, "export.md", &base);
        let changed = base.replacen("line 250\n", "line 250 changed\n", 1);
        let blocks = [write_block("sdlc/features/export.md", &changed)];

        let proposal = service.create(&blocks).await.expect("the proposal");
        let change = &proposal.changes[0];

        assert_eq!(change.action, Action::Modify);
        assert!(change.diff.contains("+line 250 changed"));
        assert!(change.diff.contains("-line 250"));
    }
}
