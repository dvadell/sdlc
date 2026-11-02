//! YAML front matter parsing (ticket 0012).
//!
//! A document may open with a YAML front matter block delimited by `---`
//! lines; the rest is Markdown body. The block is optional but, when present,
//! is small metadata: status, feature binding, ticket dependencies and decision
//! supersession.
//!
//! Parsing is best effort on purpose. A missing, malformed or invalid block
//! yields empty metadata and a warning instead of an error, so a broken metadata
//! block can never make a document unreadable or unlistable. The editor saves
//! the raw content, so unknown keys survive because the parser never rewrites.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_yaml_ng::from_str;

/// The line that opens and closes a front matter block.
const DELIMITER: &str = "---";

/// Splits `content` into its leading YAML front matter block and the body.
///
/// The opening `---` must start the document at byte 0 after an optional UTF-8
/// BOM and the closing `---` must be a line of its own, so a horizontal rule or
/// a `---` sequence deeper in the body is never mistaken for the closing marker.
/// A block without a closing marker is treated as absent.
///
/// Returns `(None, content)` when there is no front matter block, and
/// `(Some(front_matter), body)` otherwise. The body is the original text
/// byte-for-byte, including line endings and BOMs that follow the block.
pub fn split(content: &str) -> (Option<&str>, &str) {
    let bytes = content.as_bytes();
    let mut start = 0usize;
    if bytes.len() >= 3 && &bytes[..3] == b"\xEF\xBB\xBF" {
        start = 3;
    }

    let opening = &content[start..];
    let Some(opening_end) = opening.find('\n') else {
        return (None, content);
    };
    let opening_line = opening[..opening_end]
        .strip_suffix('\r')
        .unwrap_or(&opening[..opening_end]);
    if opening_line != DELIMITER {
        return (None, content);
    }

    let mut cursor = start + opening_end + 1;
    loop {
        let Some(relative) = content[cursor..].find('\n') else {
            // The block never closes, so there is no front matter to read.
            return (None, content);
        };
        let line_end = cursor + relative;
        let line = content[cursor..line_end]
            .strip_suffix('\r')
            .unwrap_or(&content[cursor..line_end]);
        if line == DELIMITER {
            let front_matter = &content[start + opening_end + 1..cursor];
            let body = &content[line_end + 1..];
            return (Some(front_matter), body);
        }
        cursor = line_end + 1;
    }
}

/// The metadata parsed from a document's front matter, typed by the status
/// vocabulary of the document kind it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocMeta<S> {
    /// The document's status, when the front matter carries a valid one.
    pub status: Option<S>,
    /// The feature this document belongs to, when the kind carries one.
    pub feature: Option<String>,
    /// The ticket ids this document depends on, as four-digit strings.
    pub depends_on: Vec<String>,
    /// The decision this document supersedes, when the kind carries one.
    pub supersedes: Option<u32>,
}

impl<S> DocMeta<S> {
    /// The metadata of a document whose front matter is absent or unusable.
    pub fn empty() -> Self {
        DocMeta {
            status: None,
            feature: None,
            depends_on: Vec::new(),
            supersedes: None,
        }
    }
}

impl<S> Default for DocMeta<S> {
    fn default() -> Self {
        Self::empty()
    }
}

/// The status an epic description may carry: `planned`, `in-progress`, `done`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EpicStatus {
    /// The epic is not started yet.
    Planned,
    /// The epic is being worked on.
    InProgress,
    /// The epic is finished.
    Done,
}

/// The status a feature may carry: `planned`, `in-progress`, `shipped`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeatureStatus {
    /// The feature is not started yet.
    Planned,
    /// The feature is being worked on.
    InProgress,
    /// The feature shipped.
    Shipped,
}

/// The status a decision may carry: `proposed`, `accepted`, `superseded`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionStatus {
    /// The decision is being drafted.
    Proposed,
    /// The decision was accepted.
    Accepted,
    /// The decision was replaced by a later decision.
    Superseded,
}

/// The status a ticket may carry: `todo`, `in-progress`, `done`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TicketStatus {
    /// The ticket was not started.
    Todo,
    /// The ticket is being worked on.
    InProgress,
    /// The ticket is done.
    Done,
}

/// The raw front matter of an epic: the kind-specific status only.
#[derive(Debug, Default, Deserialize)]
struct EpicFrontMatter {
    #[serde(default)]
    status: Option<EpicStatus>,
}

/// The raw front matter of a feature: the kind-specific status only.
#[derive(Debug, Default, Deserialize)]
struct FeatureFrontMatter {
    #[serde(default)]
    status: Option<FeatureStatus>,
}

/// The raw front matter of a decision: status plus the superseding pointer.
#[derive(Debug, Default, Deserialize)]
struct DecisionFrontMatter {
    #[serde(default)]
    status: Option<DecisionStatus>,
    #[serde(default)]
    supersedes: Option<u32>,
}

/// The raw front matter of a ticket: status, feature binding and dependencies.
#[derive(Debug, Default, Deserialize)]
struct TicketFrontMatter {
    #[serde(default)]
    status: Option<TicketStatus>,
    #[serde(default)]
    feature: Option<String>,
    #[serde(default, deserialize_with = "depends_on")]
    depends_on: Option<Vec<String>>,
}

/// Deserializes `Option<Vec<String>>` from a single quoted string, a single
/// bare number, or a list of ids.
///
/// Ticket dependencies are written quoted (`depends_on: ["0004"]`) but legacy
/// hand-written metadata uses bare numbers (`depends_on: [4, 5]` or
/// `depends_on: 7`), so all of those shapes are accepted and normalized to
/// four-digit ids.
fn depends_on<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<DependsOn>::deserialize(deserializer)?;
    Ok(value.map(|depends| depends.into_ids()))
}

/// A `depends_on` value: either a single id or a list of ids.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DependsOn {
    List(Vec<RawId>),
    Single(RawId),
}

/// One dependency id: a quoted string or a bare number.
///
/// The number is unsigned so a negative value such as `-3` is rejected instead of
/// being formatted into an invalid id like `-003`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawId {
    Text(String),
    Integer(u32),
}

impl DependsOn {
    /// Normalizes every dependency id to four digits.
    fn into_ids(self) -> Vec<String> {
        match self {
            DependsOn::List(ids) => ids.into_iter().map(RawId::id).collect(),
            DependsOn::Single(id) => vec![RawId::id(id)],
        }
    }
}

impl RawId {
    /// Normalizes a quoted id to four digits; non-numeric strings pass through.
    fn id(self) -> String {
        match self {
            RawId::Text(text) => normalize_id(&text),
            RawId::Integer(number) => format!("{:04}", number),
        }
    }
}

/// Zero-pads a normalized id to four digits; legacy bare numbers become `0004`.
fn normalize_id(value: &str) -> String {
    let trimmed = value.trim();
    trimmed
        .parse::<u32>()
        .map(|number| format!("{number:04}"))
        .unwrap_or_else(|_| trimmed.to_string())
}

/// Deserializes a front matter block, mapping any failure to `None`.
fn parse_front_matter<T>(front_matter: &str) -> Option<T>
where
    T: DeserializeOwned,
{
    from_str(front_matter).ok()
}

/// Parses the front matter of an epic description.
///
/// Missing, malformed or invalid blocks yield [`DocMeta::empty`] plus a
/// warning that names `file`; the document itself is never unreadable.
pub fn parse_epic(content: &str, file: &str) -> DocMeta<EpicStatus> {
    let (front_matter, _body) = split(content);
    let Some(front_matter) = front_matter else {
        tracing::warn!(
            file,
            "missing front matter for an epic description; using defaults"
        );
        return DocMeta::empty();
    };

    let meta = match parse_front_matter::<EpicFrontMatter>(front_matter) {
        Some(meta) => meta,
        None => {
            tracing::warn!(file, "malformed epic front matter; using defaults");
            EpicFrontMatter::default()
        }
    };

    DocMeta {
        status: meta.status,
        feature: None,
        depends_on: Vec::new(),
        supersedes: None,
    }
}

/// Parses the front matter of a feature.
///
/// Missing, malformed or invalid blocks yield [`DocMeta::empty`] plus a
/// warning that names `file`; the document itself is never unreadable.
pub fn parse_feature(content: &str, file: &str) -> DocMeta<FeatureStatus> {
    let (front_matter, _body) = split(content);
    let Some(front_matter) = front_matter else {
        tracing::warn!(file, "missing front matter for a feature; using defaults");
        return DocMeta::empty();
    };

    let meta = match parse_front_matter::<FeatureFrontMatter>(front_matter) {
        Some(meta) => meta,
        None => {
            tracing::warn!(file, "malformed feature front matter; using defaults");
            FeatureFrontMatter::default()
        }
    };

    DocMeta {
        status: meta.status,
        feature: None,
        depends_on: Vec::new(),
        supersedes: None,
    }
}

/// Parses the front matter of a decision.
///
/// Missing, malformed or invalid blocks yield [`DocMeta::empty`] plus a
/// warning that names `file`; the document itself is never unreadable.
pub fn parse_decision(content: &str, file: &str) -> DocMeta<DecisionStatus> {
    let (front_matter, _body) = split(content);
    let Some(front_matter) = front_matter else {
        tracing::warn!(file, "missing front matter for a decision; using defaults");
        return DocMeta::empty();
    };

    let meta = match parse_front_matter::<DecisionFrontMatter>(front_matter) {
        Some(meta) => meta,
        None => {
            tracing::warn!(file, "malformed decision front matter; using defaults");
            DecisionFrontMatter::default()
        }
    };

    DocMeta {
        status: meta.status,
        feature: None,
        depends_on: Vec::new(),
        supersedes: meta.supersedes,
    }
}

/// Parses the front matter of a ticket.
///
/// Missing, malformed or invalid blocks yield [`DocMeta::empty`] plus a
/// warning that names `file`; the document itself is never unreadable.
pub fn parse_ticket(content: &str, file: &str) -> DocMeta<TicketStatus> {
    let (front_matter, _body) = split(content);
    let Some(front_matter) = front_matter else {
        tracing::warn!(file, "missing front matter for a ticket; using defaults");
        return DocMeta::empty();
    };

    let meta = match parse_front_matter::<TicketFrontMatter>(front_matter) {
        Some(meta) => meta,
        None => {
            tracing::warn!(file, "malformed ticket front matter; using defaults");
            TicketFrontMatter::default()
        }
    };

    DocMeta {
        status: meta.status,
        feature: meta.feature,
        depends_on: meta.depends_on.unwrap_or_default(),
        supersedes: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_returns_no_front_matter_without_a_block() {
        assert_eq!(split("# Title\n\nbody\n"), (None, "# Title\n\nbody\n"));
    }

    #[test]
    fn split_returns_the_front_matter_and_the_body() {
        let content = "---\nstatus: planned\n---\n\n# 0001: Title\n";
        assert_eq!(
            split(content),
            (Some("status: planned\n"), "\n# 0001: Title\n")
        );
    }

    #[test]
    fn split_takes_a_bom_into_account() {
        let content = "\u{feff}---\nstatus: planned\n---\nbody";
        assert_eq!(split(content), (Some("status: planned\n"), "body"));
    }

    #[test]
    fn split_reads_crlf_front_matter() {
        let content = "---\r\nstatus: planned\r\n---\r\n\r\n# Title\r\n";
        assert_eq!(
            split(content),
            (Some("status: planned\r\n"), "\r\n# Title\r\n")
        );
    }

    #[test]
    fn split_ignores_a_horizontal_rule_as_the_closing_marker() {
        let content = "---\nstatus: planned\n---\n\n# Title\n\n---\n\nbody\n";
        assert_eq!(
            split(content),
            (Some("status: planned\n"), "\n# Title\n\n---\n\nbody\n")
        );
    }

    #[test]
    fn split_treats_an_unterminated_block_as_absent() {
        assert_eq!(
            split("---\nstatus: planned\n\n# Title\n"),
            (None, "---\nstatus: planned\n\n# Title\n")
        );
    }

    #[test]
    fn epic_parses_a_valid_status() {
        let meta = parse_epic(
            "---\nstatus: planned\n---\n\n# 0001: Title\n",
            "sdlc/epics/0001-foundation/description.md",
        );

        assert_eq!(meta.status, Some(EpicStatus::Planned));
        assert_eq!(meta.feature, None);
        assert!(meta.depends_on.is_empty());
        assert_eq!(meta.supersedes, None);
    }

    #[test]
    fn feature_parses_a_shipped_status() {
        let meta = parse_feature("---\nstatus: shipped\n---\n", "sdlc/features/export.md");

        assert_eq!(meta.status, Some(FeatureStatus::Shipped));
        assert_eq!(meta.feature, None);
    }

    #[test]
    fn decision_parses_status_and_supersedes() {
        let meta = parse_decision(
            "---\nstatus: accepted\nsupersedes: 9\n---\n",
            "sdlc/decisions/0009-use-serde-yaml-ng.md",
        );

        assert_eq!(meta.status, Some(DecisionStatus::Accepted));
        assert_eq!(meta.supersedes, Some(9));
    }

    #[test]
    fn ticket_parses_status_feature_and_dependencies() {
        let meta = parse_ticket(
            "---\nstatus: in-progress\nfeature: export\ndepends_on: [\"0004\", \"0005\"]\n---\n",
            "sdlc/epics/0001-foundation/tickets/0004-add-export.md",
        );

        assert_eq!(meta.status, Some(TicketStatus::InProgress));
        assert_eq!(meta.feature.as_deref(), Some("export"));
        assert_eq!(meta.depends_on, vec!["0004", "0005"]);
    }

    #[test]
    fn absent_front_matter_yields_empty_metadata() {
        let meta = parse_ticket(
            "no front matter at all\n",
            "sdlc/epics/0001-foundation/tickets/0001-project.md",
        );

        assert_eq!(meta.status, None);
        assert_eq!(meta.feature, None);
        assert!(meta.depends_on.is_empty());
        assert_eq!(meta.supersedes, None);
    }

    #[test]
    fn empty_front_matter_block_yields_no_status() {
        let meta = parse_epic(
            "---\n---\n\n# Title\n",
            "sdlc/epics/0001-foundation/description.md",
        );

        assert_eq!(meta.status, None);
    }

    #[test]
    fn malformed_yaml_yields_empty_metadata() {
        let meta = parse_epic(
            "---\nstatus: [unclosed\n",
            "sdlc/epics/0001-foundation/description.md",
        );

        assert_eq!(meta.status, None);
        assert_eq!(meta.feature, None);
    }

    #[test]
    fn wrong_status_type_yields_empty_metadata() {
        let meta = parse_epic(
            "---\nstatus: 3\n---\n",
            "sdlc/epics/0001-foundation/description.md",
        );

        assert_eq!(meta.status, None);
    }

    #[test]
    fn status_values_are_case_sensitive() {
        let meta = parse_epic(
            "---\nstatus: Planned\n---\n",
            "sdlc/epics/0001-foundation/description.md",
        );

        // `Planned` is not a valid kind value; values are not lowercased.
        assert_eq!(meta.status, None);
    }

    #[test]
    fn unknown_status_yields_empty_metadata() {
        // `shipped` is a valid feature status; a decision does not accept it.
        let meta = parse_decision(
            "---\nstatus: shipped\n---\n",
            "sdlc/decisions/0009-use-serde-yaml-ng.md",
        );

        assert_eq!(meta.status, None);
    }

    #[test]
    fn unknown_key_is_ignored_and_not_exposed() {
        let meta = parse_epic(
            "---\nstatus: planned\nepic: 0001-foundation\n---\n",
            "sdlc/epics/0001-foundation/description.md",
        );

        assert_eq!(meta.status, Some(EpicStatus::Planned));
        assert_eq!(meta.feature, None);
    }

    #[test]
    fn duplicate_status_key_yields_empty_metadata() {
        let meta = parse_epic(
            "---\nstatus: planned\nstatus: done\n---\n",
            "sdlc/epics/0001-foundation/description.md",
        );

        // A YAML mapping with two status keys is invalid, so the block falls
        // back to defaults rather than picking one silently.
        assert_eq!(meta.status, None);
    }

    #[test]
    fn crlf_and_bom_documents_parse() {
        let meta = parse_epic(
            "\u{feff}---\r\nstatus: done\r\n---\r\n\r\n# Title\r\n",
            "sdlc/epics/0001-foundation/description.md",
        );

        assert_eq!(meta.status, Some(EpicStatus::Done));
    }

    #[test]
    fn quoted_and_legacy_dependency_ids_normalize_to_four_digits() {
        let single = parse_ticket(
            "---\nstatus: todo\ndepends_on: \"0007\"\n---\n",
            "sdlc/epics/0001-foundation/tickets/0007-add-export.md",
        );
        assert_eq!(single.depends_on, vec!["0007"]);

        let legacy = parse_ticket(
            "---\nstatus: todo\ndepends_on: [4, 5]\n---\n",
            "sdlc/epics/0001-foundation/tickets/0005-add-import.md",
        );
        assert_eq!(legacy.depends_on, vec!["0004", "0005"]);
    }

    #[test]
    fn bare_unquoted_single_dependency_id_is_normalized() {
        let meta = parse_ticket(
            "---\nstatus: todo\ndepends_on: 7\n---\n",
            "sdlc/epics/0001-foundation/tickets/0007-add-export.md",
        );

        assert_eq!(meta.depends_on, vec!["0007"]);
        assert_eq!(meta.status, Some(TicketStatus::Todo));
    }

    #[test]
    fn negative_dependency_ids_are_rejected_as_malformed() {
        let meta = parse_ticket(
            "---\nstatus: todo\ndepends_on: [-3]\n---\n",
            "sdlc/epics/0001-foundation/tickets/0004-add-export.md",
        );

        // A negative id is not a valid four-digit ticket id, so the whole
        // block falls back to defaults instead of leaking a malformed id.
        assert_eq!(meta.status, None);
        assert!(meta.depends_on.is_empty());
    }

    #[test]
    fn dependency_lists_accept_comments_and_quoted_ids() {
        let meta = parse_ticket(
            "---\nstatus: todo\n# the next ticket\n# another comment\ndepends_on: [\"0004\", \"0005\"]\n---\n",
            "sdlc/epics/0001-foundation/tickets/0004-add-export.md",
        );

        assert_eq!(meta.depends_on, vec!["0004", "0005"]);
        assert_eq!(meta.status, Some(TicketStatus::Todo));
    }

    #[test]
    fn parsing_does_not_rewrite_the_input() {
        let content = "---\nstatus: planned\n---\n\n# Title\n";
        let (_, body) = split(content);
        assert_eq!(body, "\n# Title\n");

        let meta = parse_epic(content, "sdlc/epics/0001-foundation/description.md");
        assert_eq!(meta.status, Some(EpicStatus::Planned));
        assert_eq!(content, "---\nstatus: planned\n---\n\n# Title\n");
    }
}
