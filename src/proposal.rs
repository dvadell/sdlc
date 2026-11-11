//! Proposed file writes and deletes turned into reviewable proposals (tickets
//! 0041 and 0042).
//!
//! The block parser turns a model reply into [`Block`]s; this module turns the
//! accepted blocks into a [`Proposal`] that carries the classified change, the
//! content hash the proposal was based on and the unified diff a reviewer reads
//! before accepting, and applies or discards that proposal on accept or reject.

use std::collections::HashSet;
use std::path::{Component, Path};
use std::sync::{Arc, Mutex};

use similar::TextDiff;
use thiserror::Error;

use crate::chat::{Block, TicketContext};
use crate::store::{DocKind, DocPath, DocStore, Expectation, StoreError};
use uuid::Uuid;

/// The project-relative path of the PRD the feature index is rendered into.
const PRD_PATH: &str = "sdlc/PRD.md";

/// How many context lines the unified diff keeps around each change.
const DIFF_CONTEXT_LINES: usize = 3;

/// The last valid four-digit ticket number; a proposal that would allocate
/// above it fails instead of numbering a ticket with five digits (ticket 0050).
const MAX_TICKET_NUMBER: u32 = 9999;

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

    /// The proposal no longer matches the documents it was based on, so it
    /// cannot be applied until it is regenerated.
    #[error("the proposal conflicts with the current documents")]
    Conflict,

    /// The proposal id is not known to this service.
    #[error("the proposal is not known: {0}")]
    NotFound(String),

    /// Another accept of the same proposal is already applying it.
    #[error("the proposal accept is already in progress")]
    InProgress,

    /// The apply stopped midway: the changes before the failure were applied and
    /// the failing change is reported, so the accept never pretends to be atomic.
    #[error(
        "the proposal apply stopped midway: applied {applied:?}, failed {failed}, reason {reason}"
    )]
    Partial {
        /// The paths that were applied before the failure, in block order.
        applied: Vec<String>,
        /// The path of the change that failed.
        failed: String,
        /// The reason the failing change was not applied.
        reason: String,
    },
}

/// The outcome of an accepted proposal: what was applied and the index warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptReport {
    /// The paths that were written or deleted, in block order.
    pub applied: Vec<String>,
    /// Warnings from refreshing the PRD feature index after the apply; empty
    /// when no feature was written or deleted, and when the proposal carried
    /// the PRD itself and the refresh was skipped.
    pub warnings: Vec<String>,
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
    /// The warnings of the proposal build, such as a removed `epic` front
    /// matter key of a ticket create; empty when the build was clean.
    pub warnings: Vec<String>,
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
    /// The proposal ids an accept is currently applying, so a second accept of
    /// the same proposal fails while the first one runs.
    accepts: Mutex<std::collections::HashSet<String>>,
}

impl ProposalService {
    /// Creates a service for one store; proposals live only in memory and die
    /// with the process.
    pub fn new(store: Arc<DocStore>) -> Self {
        Self {
            store,
            proposals: Mutex::new(std::collections::HashMap::new()),
            accepts: Mutex::new(std::collections::HashSet::new()),
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
    /// When the ticket generation context of ticket 0049 is given, a proposed
    /// ticket must live in the epic the person chose, so the path has to sit under
    /// that epic's `tickets/` directory; the path, not the front matter, selects
    /// the epic (tickets 0048 and 0049).
    ///
    /// A ticket-generation create is numbered before the path is parsed (ticket
    /// 0050): the server allocates the next global ticket number, rewrites the path
    /// onto it and numbers the H1, so the model never picks a number. Existing
    /// modifications and deletes are not renumbered, and a proposal without a
    /// ticket context keeps the plain behaviour of ticket 0041.
    ///
    /// The proposal is stored in memory and returned; it only becomes real when
    /// a later ticket accepts it.
    pub async fn create(
        &self,
        blocks: &[Block],
        ticket_context: Option<&TicketContext>,
    ) -> Result<Proposal, ProposalError> {
        let mut changes: Vec<Change> = Vec::new();
        let mut seen: HashSet<DocPath> = HashSet::new();
        let mut warnings: Vec<String> = Vec::new();
        // The next ticket number to hand out for this proposal (ticket 0050): it
        // is seeded from the store on the first allocation and then runs through
        // the creates of the proposal, so the allocated numbers come out
        // sequential while nothing is reserved across proposals.
        let mut next_ticket: Option<u32> = None;

        for block in blocks {
            let raw_path = block_path(block).to_string();
            let content: Option<String> = match block {
                Block::Write { content, .. } => Some(content.clone()),
                Block::Delete { .. } => None,
            };

            // Ticket numbering runs before the path parsing and duplicate
            // rejection of ticket 0041 (ticket 0050): a ticket-generation create
            // is numbered first and then validated like any other block.
            let (raw_path, mut content) = if let Some(context) = ticket_context
                && let Some(block_content) = content.as_deref()
            {
                match self
                    .number_ticket_create(
                        &raw_path,
                        block_content,
                        context,
                        &mut next_ticket,
                        &mut warnings,
                    )
                    .await?
                {
                    Some((path, new_content)) => (path, Some(new_content)),
                    None => (raw_path, content),
                }
            } else {
                (raw_path, content)
            };

            let path = DocPath::parse(&raw_path)
                .map_err(|_| ProposalError::InvalidPath(raw_path.clone()))?;
            if !seen.insert(path.clone()) {
                return Err(ProposalError::DuplicatePath(raw_path.clone()));
            }

            if let Some(context) = ticket_context {
                // A ticket drafted in a ticket-generation turn belongs to the epic
                // the person chose, so a proposal outside that epic is refused
                // before anything is classified or stored.
                if crate::store::layout::kind(path.as_path()) == Some(DocKind::Ticket)
                    && let Some(selected) = context.epic.as_deref()
                    && crate::store::layout::epic_of(path.as_path()) != Some(selected)
                {
                    return Err(ProposalError::InvalidPath(raw_path.clone()));
                }
            }

            let (action, new_content, base_hash, base_content) = match content.take() {
                Some(content) => {
                    let document = self.store.read(&path).await;
                    match document {
                        Ok(document) => (
                            Action::Modify,
                            Some(content),
                            Some(document.hash),
                            document.content,
                        ),
                        // A missing file is a create; the base hash of a create is
                        // `None` because there is no content the proposal replaced.
                        Err(StoreError::NotFound(_)) => {
                            (Action::Create, Some(content), None, String::new())
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                None => {
                    let document = self.store.read(&path).await;
                    match document {
                        Ok(document) => {
                            (Action::Delete, None, Some(document.hash), document.content)
                        }
                        Err(StoreError::NotFound(_)) => {
                            return Err(ProposalError::MissingTarget(raw_path.clone()));
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
            warnings,
        };
        self.proposals
            .lock()
            .expect("the proposal map lock should not be poisoned")
            .insert(proposal.id.clone(), proposal.clone());

        Ok(proposal)
    }

    /// Numbers one proposed ticket create of a ticket-generation turn (ticket
    /// 0050).
    ///
    /// The ticket context of ticket 0049 turns every proposed ticket into a
    /// draft of the feature the person chose, so the server owns the ticket
    /// number: a create whose raw path sits under the selected epic's
    /// `tickets/` directory (or uses the prompt's `sdlc/epics/<epic>/tickets/`
    /// placeholder) is rewritten onto the next free number, with the content
    /// numbered and its front matter cleaned up. Modifications and deletes are
    /// left alone, and blocks that are not ticket creates of the selected epic
    /// pass through untouched, so the path validation of ticket 0041 still
    /// rejects the rest of the proposal.
    ///
    /// The number is handed out from `next_number`, which the caller seeds from
    /// the store and keeps advancing, so the creates of one proposal come out
    /// sequential while nothing is reserved across proposals.
    ///
    /// Returns the rewritten path and content, or `None` when the block is not
    /// such a create and keeps the raw path and content.
    async fn number_ticket_create(
        &self,
        raw_path: &str,
        content: &str,
        context: &TicketContext,
        next_number: &mut Option<u32>,
        warnings: &mut Vec<String>,
    ) -> Result<Option<(String, String)>, ProposalError> {
        let Some((epic, ticket_name)) = ticket_segments(raw_path) else {
            return Ok(None);
        };
        if context.epic.is_none() || (context.epic.as_deref() != Some(epic) && epic != "<epic>") {
            // The draft belongs to another epic, so numbering must not rewrite
            // it into the selected one; the path check of ticket 0041 refuses it.
            // Without a chosen epic there is nothing to allocate a number
            // against, so the block keeps its raw path and the path validation
            // of the ticket answers it.
            return Ok(None);
        }

        let allocate = match crate::store::layout::numbered_number(ticket_name) {
            // A numbered path that still exists is a modification of an existing
            // ticket, which ticket 0050 never renumbers; a numbered create is
            // rewritten onto the next free number, so the allocation runs while
            // the numbered file is still missing.
            Some(number) if number != 0 => {
                let path = DocPath::parse(raw_path)
                    .map_err(|_| ProposalError::InvalidPath(raw_path.to_string()))?;
                matches!(self.store.read(&path).await, Err(StoreError::NotFound(_)))
            }
            _ => true,
        };
        if !allocate {
            return Ok(None);
        }

        let number = match *next_number {
            Some(number) => number,
            None => {
                let number = self
                    .store
                    .next_ticket_number()
                    .await
                    .map_err(ProposalError::from)?;
                *next_number = Some(number);
                number
            }
        };
        if number > MAX_TICKET_NUMBER {
            return Err(ProposalError::Store(StoreError::NumberExhausted {
                next: number,
            }));
        }
        *next_number = Some(number + 1);

        let title = ticket_title(content, ticket_name).map_err(ProposalError::from)?;
        let slug = crate::store::templates::slugify(&title).map_err(ProposalError::from)?;
        let new_path = format!(
            "sdlc/epics/{}/tickets/{number:04}-{slug}.md",
            context.epic.as_deref().unwrap_or(epic)
        );

        let (new_content, removed_epic) =
            rewrite_ticket(content, number, &title, context.feature.as_deref());
        if removed_epic {
            warnings.push(format!(
                "removed the `epic` front matter key of {new_path}; the ticket path determines the epic"
            ));
        }

        Ok(Some((new_path, new_content)))
    }

    /// Returns the stored proposal, or `None` when the id is unknown.
    pub fn get(&self, id: &str) -> Option<Proposal> {
        self.proposals
            .lock()
            .expect("the proposal map lock should not be poisoned")
            .get(id)
            .cloned()
    }

    /// Applies a proposal the person accepted, or reports why it cannot be
    /// applied (ticket 0042).
    ///
    /// Every change is re-checked against the content the store holds now: a
    /// `Modify` or `Delete` must still hash to the proposal's base hash and a
    /// `Create` must still be missing, otherwise the accept answers
    /// [`ProposalError::Conflict`] and writes nothing at all. The changes that
    /// pass are applied in block order through the store's atomic write/delete
    /// operations, which never write without a passing expectation, so a stale
    /// file cannot be clobbered mid-apply.
    ///
    /// A write that fails stops the apply and answers [`ProposalError::Partial`]
    /// with the paths applied so far and the failing path, and the proposal is
    /// kept, so the person can inspect or reject it until it is regenerated.
    ///
    /// A successful apply removes the proposal and refreshes the PRD feature
    /// index when a feature was written or deleted; a proposal that carries the
    /// PRD itself skips that refresh, so the proposed PRD content wins.
    pub async fn accept(&self, id: &str) -> Result<AcceptReport, ProposalError> {
        {
            let mut accepts = self
                .accepts
                .lock()
                .expect("the proposal accept lock should not be poisoned");
            if accepts.contains(id) {
                return Err(ProposalError::InProgress);
            }
            accepts.insert(id.to_string());
        }

        struct AcceptGuard<'a> {
            accepts: &'a Mutex<std::collections::HashSet<String>>,
            id: String,
        }

        impl Drop for AcceptGuard<'_> {
            fn drop(&mut self) {
                // Recover the guard from a poisoned lock so the id is still
                // cleared, matching the crate's other lock sites that unwrap.
                let mut accepts = self
                    .accepts
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                accepts.remove(&self.id);
            }
        }

        let guard = AcceptGuard {
            accepts: &self.accepts,
            id: id.to_string(),
        };
        let outcome = self.apply_proposal(id).await;

        // Dropping the guard clears the accept marker before the outcome is
        // returned to the caller.
        drop(guard);
        outcome
    }

    /// Discards a proposal without touching the documents on disk (ticket 0042).
    ///
    /// Unknown ids are refused with [`ProposalError::NotFound`]; a known
    /// proposal is dropped from the in-memory map and the documents on disk are
    /// left alone.
    pub fn reject(&self, id: &str) -> Result<(), ProposalError> {
        let mut proposals = self
            .proposals
            .lock()
            .expect("the proposal map lock should not be poisoned");
        if proposals.remove(id).is_none() {
            return Err(ProposalError::NotFound(id.to_string()));
        }

        Ok(())
    }

    /// Applies one proposal that was accepted, after the accept guard was taken.
    ///
    /// The proposal is re-checked change by change, then applied in block order;
    /// see [`ProposalService::accept`] for the conflict and partial rules.
    async fn apply_proposal(&self, id: &str) -> Result<AcceptReport, ProposalError> {
        let proposal = self
            .proposals
            .lock()
            .expect("the proposal map lock should not be poisoned")
            .get(id)
            .cloned()
            .ok_or_else(|| ProposalError::NotFound(id.to_string()))?;

        // Re-check every change before touching the disk, so a stale proposal
        // never writes anything at all.
        for change in &proposal.changes {
            self.check_change(change).await?;
        }

        let mut applied: Vec<String> = Vec::new();
        for change in &proposal.changes {
            if let Err(error) = self.apply_change(change).await {
                // No transaction exists: report what was applied and stop, and
                // keep the proposal so the person can inspect or reject it.
                return Err(ProposalError::Partial {
                    applied: applied.clone(),
                    failed: path_string(&change.path),
                    reason: error.to_string(),
                });
            }
            applied.push(path_string(&change.path));
        }

        let warnings = self.feature_refresh_warnings_for(proposal).await;

        self.proposals
            .lock()
            .expect("the proposal map lock should not be poisoned")
            .remove(id);

        Ok(AcceptReport { applied, warnings })
    }

    /// Re-checks one proposal change against the current store content.
    ///
    /// A `Modify` or `Delete` must still hash to the proposal's base hash, and
    /// a `Create` must still be missing; anything else is a conflict, so the
    /// accept writes nothing until the proposal is regenerated.
    async fn check_change(&self, change: &Change) -> Result<(), ProposalError> {
        match change.action {
            Action::Create => match self.store.read(&change.path).await {
                Ok(_) => Err(ProposalError::Conflict),
                Err(StoreError::NotFound(_)) => Ok(()),
                Err(error) => Err(error.into()),
            },
            Action::Modify | Action::Delete => {
                let document = match self.store.read(&change.path).await {
                    Ok(document) => document,
                    // The file vanished after the proposal was built, so the
                    // proposal is stale and must not be applied.
                    Err(StoreError::NotFound(_)) => return Err(ProposalError::Conflict),
                    Err(error) => return Err(error.into()),
                };
                if document.hash != change.base_hash.as_deref().unwrap_or_default() {
                    return Err(ProposalError::Conflict);
                }
                Ok(())
            }
        }
    }

    /// Applies one already-checked change through the store's atomic operations.
    async fn apply_change(&self, change: &Change) -> Result<(), ProposalError> {
        match change.action {
            Action::Create => {
                let content = change.new_content.clone().unwrap_or_default();
                self.store
                    .write(&change.path, &content, &Expectation::Missing)
                    .await?;
            }
            Action::Modify => {
                let content = change.new_content.clone().unwrap_or_default();
                let base_hash = change.base_hash.clone().unwrap_or_default();
                self.store
                    .write(&change.path, &content, &Expectation::Existing(base_hash))
                    .await?;
            }
            Action::Delete => {
                // The DeleteReport (including orphaned tickets) is not surfaced
                // here; see ProposalError::Partial for the apply-failure reporting.
                self.store.delete(&change.path).await.map(|_| ())?;
            }
        }

        Ok(())
    }

    /// Refreshes the PRD feature index after a successful apply, unless the
    /// proposal carried the PRD itself and the proposed content must win.
    async fn feature_refresh_warnings_for(&self, proposal: Proposal) -> Vec<String> {
        if proposal_touches_prd(&proposal) {
            // The proposal replaced the PRD, so its content is the proposal.
            return Vec::new();
        }
        if proposal
            .changes
            .iter()
            .any(|change| change_kind(&change.path) == Some(DocKind::Feature))
        {
            return self.store.refresh_feature_index().await;
        }

        Vec::new()
    }
}

/// The path a block acts on, as written between the markers.
fn block_path(block: &Block) -> &str {
    match block {
        Block::Write { path, .. } | Block::Delete { path } => path,
    }
}

/// The project-relative path of a change, as a string for reports and results.
fn path_string(path: &DocPath) -> String {
    path.as_path().to_string_lossy().into_owned()
}

/// Whether a change acts on the PRD itself, which the proposal may propose
/// verbatim instead of refreshing through the feature index.
fn is_prd_change(path: &DocPath) -> bool {
    path.as_path() == Path::new(PRD_PATH)
}

/// The kind group a change acts on, or `None` for paths that are not documents.
fn change_kind(path: &DocPath) -> Option<crate::store::DocKind> {
    crate::store::layout::kind(path.as_path())
}

/// Whether the proposal carries its own PRD change, so the proposed content
/// must win over a feature-index refresh.
fn proposal_touches_prd(proposal: &Proposal) -> bool {
    proposal
        .changes
        .iter()
        .any(|change| is_prd_change(&change.path))
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

/// The epic directory and ticket file name of a ticket path such as
/// `sdlc/epics/0001-foundation/tickets/0001-add-export.md`, or `None` when the
/// path is not shaped like a ticket at all.
///
/// The shape is the one of the document layout: `sdlc/epics/<epic>/tickets/<name>`,
/// with the ticket file name flat inside the tickets directory.
fn ticket_segments(raw_path: &str) -> Option<(&str, &str)> {
    let mut segments: Vec<&str> = Vec::new();
    for component in Path::new(raw_path).components() {
        if let Component::Normal(segment) = component
            && let Some(segment) = segment.to_str()
        {
            segments.push(segment);
        }
    }
    if segments.len() != 5
        || segments[0] != "sdlc"
        || segments[1] != "epics"
        || segments[3] != "tickets"
    {
        return None;
    }

    Some((segments[2], segments[4]))
}

/// The title of one proposed ticket: the text of its first heading without the
/// ticket number, or the file name of the draft when the content carries no
/// heading to number.
fn ticket_title(content: &str, ticket_name: &str) -> Result<String, StoreError> {
    let (_front_matter, body) = crate::store::frontmatter::split(content);
    if let Some((_index, text)) = first_heading(body) {
        let title = strip_ticket_number(&text).trim().to_string();
        if !title.is_empty() {
            return Ok(title);
        }
    }

    // A draft without a usable heading still names its ticket, so the file name
    // carries the slug the model proposed.
    let stem = ticket_name.strip_suffix(".md").unwrap_or(ticket_name);

    Ok(strip_ticket_number(stem).trim().replace('-', " "))
}

/// The first ATX heading of a document body: the index of its line and its text
/// without the leading hashes, or `None` when the body opens with no heading.
///
/// The first heading wins whatever level it carries, so a draft that opens with a
/// deeper heading still gets its H1 normalized onto the allocated number.
fn first_heading(body: &str) -> Option<(usize, String)> {
    for (index, line) in body.split('\n').enumerate() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('#') {
            continue;
        }
        let level = trimmed.len() - trimmed.trim_start_matches('#').len();
        let text = &trimmed[level..];
        // A heading needs a space after the hashes and non-empty text; a
        // `#Title` without the space or a bare `###` is not a heading.
        if text.is_empty() || !text.starts_with(' ') {
            continue;
        }
        return Some((index, text.trim().to_string()));
    }

    None
}

/// Strips a leading ticket number from heading or file name text, such as the
/// `0007:` of `# 0007: Add export` or the `0001-` of `0001-add-export`, leaving
/// the human title or slug behind.
fn strip_ticket_number(text: &str) -> &str {
    let text = text.trim();
    let (number, rest) = if let Some((number, rest)) = text.split_once(':') {
        (number.trim(), Some(rest))
    } else if let Some((number, rest)) = text.split_once('-') {
        (number, Some(rest))
    } else {
        (text, None)
    };
    let is_number =
        !number.is_empty() && number.len() <= 4 && number.bytes().all(|byte| byte.is_ascii_digit());
    if is_number {
        return rest.map(str::trim).unwrap_or("");
    }

    text
}

/// The rewritten content of one numbered ticket create, with its front matter
/// cleaned up (ticket 0050).
///
/// The first heading of the body becomes `# <number>: <title>`, any `epic` front
/// matter key the model added is dropped and the optional feature binding is set
/// to the selected feature; unknown keys and the rest of the document survive.
fn rewrite_ticket(
    content: &str,
    number: u32,
    title: &str,
    feature: Option<&str>,
) -> (String, bool) {
    let (front_matter, body) = crate::store::frontmatter::split(content);

    let new_body = match first_heading(body) {
        Some((index, _text)) => {
            let heading = format!("# {number:04}: {title}");
            let mut lines: Vec<&str> = body.split('\n').collect();
            lines[index] = heading.as_str();
            lines.join("\n")
        }
        None => body.to_string(),
    };

    let mut front_matter_text = None;
    let mut removed_epic = false;
    if let Some(front_matter) = front_matter {
        let mut lines: Vec<String> = front_matter
            .split('\n')
            .map(|line| line.trim_end_matches('\r').to_string())
            .collect();
        if lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        let before = lines.len();
        lines.retain(|line| front_key(line) != Some("epic"));
        if lines.len() < before {
            removed_epic = true;
        }
        if let Some(feature) = feature {
            // The ticket context owns the feature binding, so an existing value
            // is replaced when it disagrees with the selected feature and the key
            // is added when the draft did not carry one.
            let mut replaced = false;
            let mut rewritten: Vec<String> = Vec::with_capacity(lines.len());
            for line in lines.drain(..) {
                if front_key(&line) == Some("feature") {
                    rewritten.push(format!("feature: {feature}"));
                    replaced = true;
                } else {
                    rewritten.push(line);
                }
            }
            if !replaced {
                rewritten.push(format!("feature: {feature}"));
            }
            lines = rewritten;
        }
        // The front matter is re-emitted without its trailing newline, so the
        // joining below renders the block delimiter right.
        front_matter_text = Some(lines.join("\n"));
    } else if let Some(feature) = feature {
        front_matter_text = Some(format!("feature: {feature}"));
    }

    let body = if let Some(stripped) = new_body.strip_prefix('\n') {
        stripped.to_string()
    } else {
        new_body
    };
    let new_content = match front_matter_text {
        Some(front_matter) => format!("---\n{front_matter}\n---\n\n{body}"),
        None => body,
    };

    (new_content, removed_epic)
}

/// The key of one front matter line, without its value.
fn front_key(line: &str) -> Option<&str> {
    let line = line.trim();
    let key = line.split_once(':')?.0.trim();
    // Anchor on the key itself, after an optional YAML list marker, so a list
    // entry such as `- feature: x` reads as the `feature` key (ticket 0050).
    Some(key.strip_prefix("- ").unwrap_or(key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Block;
    use crate::store::{DocStore, content_hash};
    use std::sync::Arc;
    use tempfile::TempDir;

    /// The starter PRD of a scaffolded project, with the marked region still empty.
    const PRD_WITH_MARKERS: &str = "# Product Requirements Document\n\n## Purpose\n\nTODO: write the purpose of this document in one or two short paragraphs.\n\n<!-- sdlc:feature-index:start -->\n<!-- sdlc:feature-index:end -->\n";

    async fn service_in_temp() -> (TempDir, ProposalService) {
        let (dir, _store, service) = service_in_temp_with_store().await;

        (dir, service)
    }

    async fn service_in_temp_with_store() -> (TempDir, Arc<DocStore>, ProposalService) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(DocStore::new(dir.path().to_path_buf()).expect("store"));
        let service = ProposalService::new(store.clone());

        (dir, store, service)
    }

    fn write_file(dir: &TempDir, relative: &str, content: &str) -> std::path::PathBuf {
        let path = dir.path().join(relative);
        std::fs::create_dir_all(path.parent().expect("the parent dir")).expect("parent dir");
        std::fs::write(&path, content).expect("write the document");
        path
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

    /// The test process runs with the effective user id 0, which bypasses the
    /// read-only directory permissions a failure-path test relies on.
    #[cfg(unix)]
    fn running_as_root() -> bool {
        let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
            return false;
        };
        status.lines().any(|line| {
            let mut fields = line.split_whitespace();
            if fields.next() == Some("Uid:") {
                // `Uid: real effective saved fs`, so the effective uid is the
                // one that decides whether the read-only permissions are bypassed.
                return fields.nth(1) == Some("0");
            }
            false
        })
    }

    #[tokio::test]
    async fn empty_block_list_is_rejected() {
        let (_dir, service) = service_in_temp().await;

        let error = service.create(&[], None).await.expect_err("empty proposal");

        assert!(matches!(error, ProposalError::EmptyProposal));
    }

    #[tokio::test]
    async fn write_to_a_missing_file_is_a_create() {
        let (_dir, service) = service_in_temp().await;
        let blocks = [write_block("sdlc/features/export.md", "# Export\n")];

        let proposal = service.create(&blocks, None).await.expect("the proposal");

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

        let proposal = service.create(&blocks, None).await.expect("the proposal");

        let change = &proposal.changes[0];
        assert_eq!(change.action, Action::Modify);
        assert_eq!(
            change.base_hash.as_deref(),
            Some(content_hash(b"# Export\n").as_str())
        );
        assert_eq!(change.new_content.as_deref(), Some("# Exported\n"));
    }

    #[tokio::test]
    async fn write_to_an_existing_agents_file_is_a_modify_with_the_base_hash_and_diff() {
        let (dir, service) = service_in_temp().await;
        write_file(&dir, "AGENTS.md", "# sdlc\n");
        let blocks = [write_block(
            "AGENTS.md",
            "# sdlc\n\n## Commands\n\n- run the tests\n",
        )];

        let proposal = service.create(&blocks, None).await.expect("the proposal");

        let change = &proposal.changes[0];
        assert_eq!(change.action, Action::Modify);
        assert_eq!(
            change.base_hash.as_deref(),
            Some(content_hash(b"# sdlc\n").as_str())
        );
        assert_eq!(
            change.new_content.as_deref(),
            Some("# sdlc\n\n## Commands\n\n- run the tests\n")
        );
        assert!(change.diff.contains("--- a/AGENTS.md"));
        assert!(change.diff.contains("+++ b/AGENTS.md"));
        assert!(change.diff.contains("+## Commands"));
        assert!(change.diff.contains("+- run the tests"));
    }

    #[tokio::test]
    async fn delete_requires_an_existing_file() {
        let (_dir, service) = service_in_temp().await;

        let error = service
            .create(&[delete_block("sdlc/features/nope.md")], None)
            .await
            .expect_err("deleting a missing file");

        assert!(matches!(error, ProposalError::MissingTarget(_)));
    }

    #[tokio::test]
    async fn delete_of_an_existing_file_is_a_delete_with_the_base_hash() {
        let (dir, service) = service_in_temp().await;
        feature_file(&dir, "export.md", "# Export\n");
        let blocks = [delete_block("sdlc/features/export.md")];

        let proposal = service.create(&blocks, None).await.expect("the proposal");

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

        let error = service
            .create(&blocks, None)
            .await
            .expect_err("invalid proposal");

        assert!(matches!(error, ProposalError::InvalidPath(_)));
    }

    #[tokio::test]
    async fn a_proposal_for_a_rules_file_the_tool_does_not_manage_is_rejected() {
        // `CLAUDE.md` is not one of the allowed document paths, so the whole
        // proposal is refused before any change is classified or stored.
        let (_dir, service) = service_in_temp().await;
        let blocks = [
            write_block("AGENTS.md", "# Rules\n"),
            write_block("CLAUDE.md", "# Rules\n"),
        ];

        let error = service
            .create(&blocks, None)
            .await
            .expect_err("the proposal should be rejected");

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
                .create(&[write_block(raw, "# No\n")], None)
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
            .create(&blocks, None)
            .await
            .expect_err("duplicate proposal");

        assert!(matches!(error, ProposalError::DuplicatePath(_)));
    }

    #[tokio::test]
    async fn same_content_yields_an_empty_diff_but_a_valid_proposal() {
        let (dir, service) = service_in_temp().await;
        feature_file(&dir, "export.md", "# Export\n");
        let blocks = [write_block("sdlc/features/export.md", "# Export\n")];

        let proposal = service.create(&blocks, None).await.expect("the proposal");

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
            .create(&[write_block("sdlc/PRD.md", "# First\n")], None)
            .await
            .expect("first proposal");
        let second = service
            .create(&[write_block("sdlc/PRD.md", "# Second\n")], None)
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

        let proposal = service.create(&blocks, None).await.expect("the proposal");
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
            .create(
                &[write_block(
                    "sdlc/features/export.md",
                    "# Export\n\nHello\n",
                )],
                None,
            )
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
            .create(&[delete_block("sdlc/features/export.md")], None)
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

        let proposal = service.create(&blocks, None).await.expect("the proposal");
        let change = &proposal.changes[0];

        assert_eq!(change.action, Action::Modify);
        assert!(change.diff.contains("+line 250 changed"));
        assert!(change.diff.contains("-line 250"));
    }

    #[tokio::test]
    async fn accept_applies_creates_modifies_and_deletes() {
        let (dir, _store, service) = service_in_temp_with_store().await;
        feature_file(&dir, "export.md", "# Export\n");
        feature_file(&dir, "old.md", "# Old\n");
        let blocks = [
            write_block("sdlc/features/export.md", "# Exported\n"),
            write_block("sdlc/features/v2-api.md", "# V2 API\n"),
            delete_block("sdlc/features/old.md"),
        ];

        let proposal = service.create(&blocks, None).await.expect("the proposal");
        let report = service.accept(&proposal.id).await.expect("the accept");

        assert_eq!(
            report.applied,
            vec![
                "sdlc/features/export.md".to_string(),
                "sdlc/features/v2-api.md".to_string(),
                "sdlc/features/old.md".to_string(),
            ]
        );

        assert_eq!(
            std::fs::read_to_string(dir.path().join("sdlc/features/export.md"))
                .expect("read the feature"),
            "# Exported\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("sdlc/features/v2-api.md"))
                .expect("read the feature"),
            "# V2 API\n"
        );
        assert!(
            !dir.path().join("sdlc/features/old.md").exists(),
            "the deleted feature must be gone"
        );
    }

    #[tokio::test]
    async fn accept_of_a_stale_hash_writes_nothing_and_keeps_the_proposal() {
        let (dir, _store, service) = service_in_temp_with_store().await;
        feature_file(&dir, "export.md", "# Export\n");
        let blocks = [write_block("sdlc/features/export.md", "# Exported\n")];
        let proposal = service.create(&blocks, None).await.expect("the proposal");
        std::fs::write(
            dir.path().join("sdlc/features/export.md"),
            "# Changed externally\n",
        )
        .expect("rewrite the feature");

        let error = service.accept(&proposal.id).await.expect_err("the accept");

        assert!(matches!(error, ProposalError::Conflict));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("sdlc/features/export.md"))
                .expect("read the feature"),
            "# Changed externally\n"
        );
        assert!(service.get(&proposal.id).is_some());
    }

    #[tokio::test]
    async fn accept_of_a_create_whose_target_appeared_is_a_conflict() {
        let (dir, _store, service) = service_in_temp_with_store().await;
        std::fs::create_dir_all(dir.path().join("sdlc/features")).expect("feature dir");
        let blocks = [write_block("sdlc/features/export.md", "# Export\n")];
        let proposal = service.create(&blocks, None).await.expect("the proposal");
        std::fs::write(dir.path().join("sdlc/features/export.md"), "# External\n")
            .expect("write the feature");

        let error = service.accept(&proposal.id).await.expect_err("the accept");

        assert!(matches!(error, ProposalError::Conflict));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("sdlc/features/export.md"))
                .expect("read the feature"),
            "# External\n"
        );
    }

    #[tokio::test]
    async fn accept_of_a_delete_of_a_changed_file_writes_nothing() {
        let (dir, _store, service) = service_in_temp_with_store().await;
        feature_file(&dir, "export.md", "# Export\n");
        let proposal = service
            .create(&[delete_block("sdlc/features/export.md")], None)
            .await
            .expect("the proposal");
        std::fs::write(
            dir.path().join("sdlc/features/export.md"),
            "# Changed externally\n",
        )
        .expect("rewrite the feature");

        let error = service.accept(&proposal.id).await.expect_err("the accept");

        assert!(matches!(error, ProposalError::Conflict));
        assert!(
            dir.path().join("sdlc/features/export.md").exists(),
            "a conflicted delete must not remove the file"
        );
    }

    #[tokio::test]
    async fn accept_after_the_file_was_deleted_is_a_conflict() {
        let (dir, _store, service) = service_in_temp_with_store().await;
        feature_file(&dir, "export.md", "# Export\n");
        let blocks = [write_block("sdlc/features/export.md", "# Exported\n")];
        let proposal = service.create(&blocks, None).await.expect("the proposal");
        std::fs::remove_file(dir.path().join("sdlc/features/export.md")).expect("remove");

        let error = service.accept(&proposal.id).await.expect_err("the accept");

        assert!(matches!(error, ProposalError::Conflict));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn partial_apply_reports_the_applied_and_the_failing_path() {
        use std::os::unix::fs::PermissionsExt;

        // Read-only directory permissions cannot force a write failure when the
        // test process runs as root, which bypasses them.
        if running_as_root() {
            return;
        }

        let (dir, _store, service) = service_in_temp_with_store().await;
        write_file(&dir, "sdlc/PRD.md", "# Product Requirements Document\n");
        feature_file(&dir, "gamma.md", "# Gamma\n");
        let blocks = [
            write_block("sdlc/PRD.md", "# Fresh PRD\n"),
            write_block("sdlc/features/gamma.md", "# Gamma v2\n"),
        ];
        let proposal = service.create(&blocks, None).await.expect("the proposal");
        let feature_dir = dir.path().join("sdlc/features");
        std::fs::set_permissions(&feature_dir, std::fs::Permissions::from_mode(0o500))
            .expect("make the feature dir read-only");
        let error = service.accept(&proposal.id).await.expect_err("the accept");
        std::fs::set_permissions(&feature_dir, std::fs::Permissions::from_mode(0o700))
            .expect("restore the feature dir");

        let ProposalError::Partial {
            applied,
            failed,
            reason,
        } = error
        else {
            panic!("the accept should be partial, got {error:?}");
        };

        assert_eq!(applied, vec!["sdlc/PRD.md".to_string()]);
        assert_eq!(failed, "sdlc/features/gamma.md");
        assert!(!reason.is_empty());

        // The first change was applied and the second was not; the proposal is
        // kept, so it can still be inspected or rejected.
        assert_eq!(
            std::fs::read_to_string(dir.path().join("sdlc/PRD.md")).expect("read the prd"),
            "# Fresh PRD\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("sdlc/features/gamma.md"))
                .expect("read the feature"),
            "# Gamma\n"
        );
        assert!(service.get(&proposal.id).is_some());
    }

    #[tokio::test]
    async fn accept_refreshes_the_prd_feature_index_for_a_feature_change() {
        let (dir, store, service) = service_in_temp_with_store().await;
        std::fs::create_dir_all(dir.path().join("sdlc/features")).expect("feature dir");
        write_file(&dir, "sdlc/PRD.md", PRD_WITH_MARKERS);
        let blocks = [write_block("sdlc/features/export.md", "# Export\n")];

        let proposal = service.create(&blocks, None).await.expect("the proposal");
        let report = service.accept(&proposal.id).await.expect("the accept");

        assert_eq!(report.applied, vec!["sdlc/features/export.md".to_string()]);

        let document = store
            .read(&DocPath::parse("sdlc/PRD.md").expect("the prd path is valid"))
            .await
            .expect("read the prd");
        assert!(
            document
                .content
                .contains("- [Export](features/export.md) — planned"),
            "the refreshed index must list the accepted feature, got {}",
            document.content
        );
        assert!(
            document
                .content
                .contains(crate::store::index::FEATURE_INDEX_START),
            "the refreshed index must keep the feature index markers"
        );
    }

    #[tokio::test]
    async fn accept_keeps_an_explicit_prd_change_instead_of_refreshing() {
        let (dir, store, service) = service_in_temp_with_store().await;
        write_file(&dir, "sdlc/PRD.md", PRD_WITH_MARKERS);
        let proposed = "# Product Requirements Document\n\n## Purpose\n\nWrite the purpose.\n\n<!-- sdlc:feature-index:start -->\n<!-- sdlc:feature-index:end -->\n";
        let blocks = [write_block("sdlc/PRD.md", proposed)];

        let proposal = service.create(&blocks, None).await.expect("the proposal");
        let report = service.accept(&proposal.id).await.expect("the accept");

        assert!(report.warnings.is_empty());

        let document = store
            .read(&DocPath::parse("sdlc/PRD.md").expect("the prd path is valid"))
            .await
            .expect("read the prd");
        assert_eq!(document.content, proposed);
    }

    #[tokio::test]
    async fn two_ticket_creates_in_the_chosen_epic_build_a_proposal_of_creates() {
        let (dir, _store, service) = service_in_temp_with_store().await;
        feature_file(&dir, "export.md", "# Export\n");
        let context = TicketContext {
            feature: Some("export".to_string()),
            epic: Some("0001-foundation".to_string()),
        };
        let blocks = [
            write_block(
                "sdlc/epics/0001-foundation/tickets/0001-add-export.md",
                "# 0001: Add export\n",
            ),
            write_block(
                "sdlc/epics/0001-foundation/tickets/0002-add-import.md",
                "# 0002: Add import\n",
            ),
        ];

        let proposal = service
            .create(&blocks, Some(&context))
            .await
            .expect("the proposal");

        assert_eq!(proposal.changes.len(), 2);
        assert_eq!(proposal.changes[0].action, Action::Create);
        assert_eq!(
            proposal.changes[0]
                .path
                .as_path()
                .to_string_lossy()
                .into_owned(),
            "sdlc/epics/0001-foundation/tickets/0001-add-export.md"
        );
        assert_eq!(proposal.changes[1].action, Action::Create);
        assert_eq!(
            proposal.changes[1]
                .path
                .as_path()
                .to_string_lossy()
                .into_owned(),
            "sdlc/epics/0001-foundation/tickets/0002-add-import.md"
        );
        assert!(service.get(&proposal.id).is_some());
    }

    #[tokio::test]
    async fn a_ticket_draft_outside_the_chosen_epic_is_rejected() {
        let (dir, _store, service) = service_in_temp_with_store().await;
        feature_file(&dir, "export.md", "# Export\n");
        let context = TicketContext {
            feature: Some("export".to_string()),
            epic: Some("0001-foundation".to_string()),
        };
        let blocks = [write_block(
            "sdlc/epics/0002-platform/tickets/0001-add-export.md",
            "# 0001: Add export\n",
        )];

        let error = service
            .create(&blocks, Some(&context))
            .await
            .expect_err("the proposal");

        assert!(matches!(error, ProposalError::InvalidPath(_)));
    }

    // ------------------------------------------------------------------
    // Ticket 0050: server-side ticket numbering

    fn ticket_context(epic: &str, feature: Option<&str>) -> TicketContext {
        TicketContext {
            feature: feature.map(|feature| feature.to_string()),
            epic: Some(epic.to_string()),
        }
    }

    fn change_path(change: &Change) -> String {
        change.path.as_path().to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn two_unnumbered_ticket_creates_are_numbered_in_the_chosen_epic() {
        let (_dir, _store, service) = service_in_temp_with_store().await;
        let context = ticket_context("0001-foundation", Some("export"));
        let blocks = [
            write_block(
                "sdlc/epics/0001-foundation/tickets/add-export.md",
                "# Add Export\n",
            ),
            write_block(
                "sdlc/epics/0001-foundation/tickets/add-import.md",
                "# Add Import\n",
            ),
        ];

        let proposal = service
            .create(&blocks, Some(&context))
            .await
            .expect("the proposal");

        assert_eq!(proposal.changes.len(), 2);
        assert_eq!(
            change_path(&proposal.changes[0]),
            "sdlc/epics/0001-foundation/tickets/0001-add-export.md"
        );
        assert_eq!(
            change_path(&proposal.changes[1]),
            "sdlc/epics/0001-foundation/tickets/0002-add-import.md"
        );
        assert_eq!(proposal.changes[0].action, Action::Create);
        assert_eq!(proposal.changes[1].action, Action::Create);
        assert_eq!(
            proposal.changes[0].new_content.as_deref(),
            Some("---\nfeature: export\n---\n\n# 0001: Add Export\n")
        );
        assert_eq!(
            proposal.changes[1].new_content.as_deref(),
            Some("---\nfeature: export\n---\n\n# 0002: Add Import\n")
        );
    }

    #[tokio::test]
    async fn three_unnumbered_ticket_creates_number_themselves_sequentially() {
        let (_dir, _store, service) = service_in_temp_with_store().await;
        let context = ticket_context("0001-foundation", Some("export"));
        let blocks = [
            write_block(
                "sdlc/epics/0001-foundation/tickets/add-export.md",
                "# Add Export\n",
            ),
            write_block(
                "sdlc/epics/0001-foundation/tickets/add-import.md",
                "# Add Import\n",
            ),
            write_block(
                "sdlc/epics/0001-foundation/tickets/add-audit.md",
                "# Add Audit\n",
            ),
        ];

        let proposal = service
            .create(&blocks, Some(&context))
            .await
            .expect("the proposal");

        assert_eq!(proposal.changes.len(), 3);
        assert_eq!(
            change_path(&proposal.changes[0]),
            "sdlc/epics/0001-foundation/tickets/0001-add-export.md"
        );
        assert_eq!(
            change_path(&proposal.changes[1]),
            "sdlc/epics/0001-foundation/tickets/0002-add-import.md"
        );
        assert_eq!(
            change_path(&proposal.changes[2]),
            "sdlc/epics/0001-foundation/tickets/0003-add-audit.md"
        );
        assert_eq!(
            proposal.changes[0].new_content.as_deref(),
            Some("---\nfeature: export\n---\n\n# 0001: Add Export\n")
        );
        assert_eq!(
            proposal.changes[1].new_content.as_deref(),
            Some("---\nfeature: export\n---\n\n# 0002: Add Import\n")
        );
        assert_eq!(
            proposal.changes[2].new_content.as_deref(),
            Some("---\nfeature: export\n---\n\n# 0003: Add Audit\n")
        );
    }

    #[tokio::test]
    async fn a_model_supplied_ticket_number_is_replaced_by_the_next_free_number() {
        let (_dir, _store, service) = service_in_temp_with_store().await;
        let context = ticket_context("0001-foundation", Some("export"));
        let blocks = [write_block(
            "sdlc/epics/0001-foundation/tickets/0007-add-export.md",
            "# 0007: Add Export\n",
        )];

        let proposal = service
            .create(&blocks, Some(&context))
            .await
            .expect("the proposal");

        assert_eq!(
            change_path(&proposal.changes[0]),
            "sdlc/epics/0001-foundation/tickets/0001-add-export.md"
        );
        assert_eq!(
            proposal.changes[0].new_content.as_deref(),
            Some("---\nfeature: export\n---\n\n# 0001: Add Export\n")
        );
    }

    #[tokio::test]
    async fn a_ticket_create_without_a_usable_heading_numbers_from_the_file_name() {
        let (_dir, _store, service) = service_in_temp_with_store().await;
        let context = ticket_context("0001-foundation", Some("export"));
        let blocks = [write_block(
            "sdlc/epics/0001-foundation/tickets/add-export.md",
            "Some description of the export.\n",
        )];

        let proposal = service
            .create(&blocks, Some(&context))
            .await
            .expect("the proposal");

        assert_eq!(
            change_path(&proposal.changes[0]),
            "sdlc/epics/0001-foundation/tickets/0001-add-export.md"
        );
        assert_eq!(
            proposal.changes[0].new_content.as_deref(),
            Some("---\nfeature: export\n---\n\nSome description of the export.\n")
        );
    }

    #[tokio::test]
    async fn a_ticket_create_without_a_usable_heading_strips_the_model_number() {
        let (_dir, _store, service) = service_in_temp_with_store().await;
        let context = ticket_context("0001-foundation", Some("export"));
        let blocks = [write_block(
            "sdlc/epics/0001-foundation/tickets/0007-add-export.md",
            "Some description of the export.\n",
        )];

        let proposal = service
            .create(&blocks, Some(&context))
            .await
            .expect("the proposal");

        assert_eq!(
            change_path(&proposal.changes[0]),
            "sdlc/epics/0001-foundation/tickets/0001-add-export.md"
        );
        assert_eq!(
            proposal.changes[0].new_content.as_deref(),
            Some("---\nfeature: export\n---\n\nSome description of the export.\n")
        );
    }

    #[tokio::test]
    async fn a_deeper_heading_is_normalized_onto_the_allocated_h1() {
        let (_dir, _store, service) = service_in_temp_with_store().await;
        let context = ticket_context("0001-foundation", Some("export"));
        let blocks = [write_block(
            "sdlc/epics/0001-foundation/tickets/add-export.md",
            "## Add Export\n",
        )];

        let proposal = service
            .create(&blocks, Some(&context))
            .await
            .expect("the proposal");

        assert_eq!(
            change_path(&proposal.changes[0]),
            "sdlc/epics/0001-foundation/tickets/0001-add-export.md"
        );
        assert_eq!(
            proposal.changes[0].new_content.as_deref(),
            Some("---\nfeature: export\n---\n\n# 0001: Add Export\n")
        );
    }

    #[tokio::test]
    async fn the_model_slug_is_slugified_into_the_ticket_file_name() {
        let (_dir, _store, service) = service_in_temp_with_store().await;
        let context = ticket_context("0001-foundation", Some("export"));
        let blocks = [write_block(
            "sdlc/epics/0001-foundation/tickets/add-export.md",
            "# Add CSV Export\n",
        )];

        let proposal = service
            .create(&blocks, Some(&context))
            .await
            .expect("the proposal");

        assert_eq!(
            change_path(&proposal.changes[0]),
            "sdlc/epics/0001-foundation/tickets/0001-add-csv-export.md"
        );
        assert_eq!(
            proposal.changes[0].new_content.as_deref(),
            Some("---\nfeature: export\n---\n\n# 0001: Add CSV Export\n")
        );
    }

    #[tokio::test]
    async fn an_empty_ticket_title_is_rejected() {
        let (_dir, _store, service) = service_in_temp_with_store().await;
        let context = ticket_context("0001-foundation", Some("export"));
        let blocks = [write_block(
            "sdlc/epics/0001-foundation/tickets/add-export.md",
            "# !!!\n",
        )];

        let error = service
            .create(&blocks, Some(&context))
            .await
            .expect_err("the proposal");

        assert!(matches!(
            error,
            ProposalError::Store(StoreError::InvalidTitle(_))
        ));
    }

    #[tokio::test]
    async fn a_removed_epic_key_is_reported_and_the_feature_is_bound() {
        let (_dir, _store, service) = service_in_temp_with_store().await;
        let context = ticket_context("0001-foundation", Some("export"));
        let blocks = [write_block(
            "sdlc/epics/0001-foundation/tickets/add-export.md",
            "---\nepic: 0002-platform\nstatus: todo\n---\n\n# Add Export\n",
        )];

        let proposal = service
            .create(&blocks, Some(&context))
            .await
            .expect("the proposal");

        assert_eq!(proposal.warnings.len(), 1);
        assert!(proposal.warnings[0].contains("epic"));
        assert_eq!(
            proposal.changes[0].new_content.as_deref(),
            Some("---\nstatus: todo\nfeature: export\n---\n\n# 0001: Add Export\n")
        );
    }

    #[tokio::test]
    async fn a_wrong_feature_is_replaced_by_the_selected_one() {
        let (_dir, _store, service) = service_in_temp_with_store().await;
        let context = ticket_context("0001-foundation", Some("export"));
        let blocks = [write_block(
            "sdlc/epics/0001-foundation/tickets/add-export.md",
            "---\nstatus: todo\nfeature: wrong\n---\n\n# Add Export\n",
        )];

        let proposal = service
            .create(&blocks, Some(&context))
            .await
            .expect("the proposal");

        assert_eq!(
            proposal.changes[0].new_content.as_deref(),
            Some("---\nstatus: todo\nfeature: export\n---\n\n# 0001: Add Export\n")
        );
    }

    #[tokio::test]
    async fn unknown_front_matter_keys_survive_a_numbered_create() {
        let (_dir, _store, service) = service_in_temp_with_store().await;
        let context = ticket_context("0001-foundation", Some("export"));
        let blocks = [write_block(
            "sdlc/epics/0001-foundation/tickets/add-export.md",
            "---\nstatus: todo\ndepends_on: [\"0004\"]\nepic: 0002-platform\n---\n\n# Add Export\n",
        )];

        let proposal = service
            .create(&blocks, Some(&context))
            .await
            .expect("the proposal");

        let content = proposal.changes[0]
            .new_content
            .as_deref()
            .expect("the content");
        assert!(content.contains("depends_on: [\"0004\"]"));
        assert!(content.contains("status: todo"));
        assert!(!content.contains("epic:"));
    }

    #[tokio::test]
    async fn the_numbered_proposal_can_be_accepted_and_written_to_disk() {
        let (dir, _store, service) = service_in_temp_with_store().await;
        std::fs::create_dir_all(dir.path().join("sdlc/epics/0001-foundation/tickets"))
            .expect("create the epic tickets directory");
        let context = ticket_context("0001-foundation", Some("export"));
        let blocks = [write_block(
            "sdlc/epics/0001-foundation/tickets/add-export.md",
            "# Add Export\n",
        )];

        let proposal = service
            .create(&blocks, Some(&context))
            .await
            .expect("the proposal");
        let report = service.accept(&proposal.id).await.expect("the accept");

        assert_eq!(
            report.applied,
            vec!["sdlc/epics/0001-foundation/tickets/0001-add-export.md".to_string()]
        );
        assert_eq!(
            std::fs::read_to_string(
                dir.path()
                    .join("sdlc/epics/0001-foundation/tickets/0001-add-export.md")
            )
            .expect("read the ticket"),
            "---\nfeature: export\n---\n\n# 0001: Add Export\n"
        );
    }

    #[tokio::test]
    async fn a_ticket_create_past_the_four_digit_boundary_fails_cleanly() {
        let (dir, _store, service) = service_in_temp_with_store().await;
        std::fs::create_dir_all(dir.path().join("sdlc/epics/0001-foundation/tickets"))
            .expect("create the epic tickets directory");
        std::fs::write(
            dir.path()
                .join("sdlc/epics/0001-foundation/tickets/9999-late.md"),
            "# 9999: Late\n",
        )
        .expect("write the ticket");

        let context = ticket_context("0001-foundation", Some("export"));
        let blocks = [write_block(
            "sdlc/epics/0001-foundation/tickets/add-export.md",
            "# Add Export\n",
        )];

        let error = service
            .create(&blocks, Some(&context))
            .await
            .expect_err("the proposal");

        assert!(matches!(
            error,
            ProposalError::Store(StoreError::NumberExhausted { next: 10000 })
        ));
    }

    #[tokio::test]
    async fn an_existing_ticket_modify_is_not_renumbered() {
        let (dir, _store, service) = service_in_temp_with_store().await;
        write_file(
            &dir,
            "sdlc/epics/0001-foundation/tickets/0003-add-export.md",
            "# 0003: Add Export\n",
        );
        let context = ticket_context("0001-foundation", Some("export"));
        let blocks = [write_block(
            "sdlc/epics/0001-foundation/tickets/0003-add-export.md",
            "# 0003: Add Export\n",
        )];

        let proposal = service
            .create(&blocks, Some(&context))
            .await
            .expect("the proposal");

        assert_eq!(proposal.changes.len(), 1);
        assert_eq!(proposal.changes[0].action, Action::Modify);
        assert_eq!(
            change_path(&proposal.changes[0]),
            "sdlc/epics/0001-foundation/tickets/0003-add-export.md"
        );
        assert_eq!(
            proposal.changes[0].new_content.as_deref(),
            Some("# 0003: Add Export\n")
        );
    }

    #[tokio::test]
    async fn without_a_ticket_context_the_plain_proposal_behaviour_is_kept() {
        let (_dir, _store, service) = service_in_temp_with_store().await;

        let error = service
            .create(
                &[write_block(
                    "sdlc/epics/0001-foundation/tickets/add-export.md",
                    "# Add Export\n",
                )],
                None,
            )
            .await
            .expect_err("the proposal");

        assert!(matches!(error, ProposalError::InvalidPath(_)));

        let proposal = service
            .create(
                &[write_block(
                    "sdlc/epics/0001-foundation/tickets/0007-add-export.md",
                    "# 0007: Add Export\n",
                )],
                None,
            )
            .await
            .expect("the proposal");

        assert_eq!(
            change_path(&proposal.changes[0]),
            "sdlc/epics/0001-foundation/tickets/0007-add-export.md"
        );
        assert_eq!(
            proposal.changes[0].new_content.as_deref(),
            Some("# 0007: Add Export\n")
        );
    }

    #[tokio::test]
    async fn reject_does_not_touch_disk_and_drops_the_proposal() {
        let (dir, _store, service) = service_in_temp_with_store().await;
        feature_file(&dir, "export.md", "# Export\n");
        let blocks = [write_block("sdlc/features/export.md", "# Exported\n")];
        let proposal = service.create(&blocks, None).await.expect("the proposal");
        assert!(service.get(&proposal.id).is_some());

        service.reject(&proposal.id).expect("the reject");

        assert!(service.get(&proposal.id).is_none());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("sdlc/features/export.md"))
                .expect("read the feature"),
            "# Export\n"
        );
    }

    #[tokio::test]
    async fn reject_of_an_unknown_id_is_not_found() {
        let (_dir, _store, service) = service_in_temp_with_store().await;

        let error = service.reject("no-such-proposal").expect_err("the reject");

        assert!(matches!(error, ProposalError::NotFound(_)));
    }

    #[tokio::test]
    async fn a_second_accept_of_the_same_id_fails_cleanly() {
        let (_dir, _store, service) = service_in_temp_with_store().await;
        std::fs::create_dir_all(_dir.path().join("sdlc/features")).expect("feature dir");
        let blocks = [write_block("sdlc/features/export.md", "# Export\n")];
        let proposal = service.create(&blocks, None).await.expect("the proposal");
        let report = service.accept(&proposal.id).await.expect("the accept");
        assert_eq!(report.applied, vec!["sdlc/features/export.md".to_string()]);

        let error = service
            .accept(&proposal.id)
            .await
            .expect_err("the second accept");

        assert!(matches!(error, ProposalError::NotFound(_)));
    }
}
