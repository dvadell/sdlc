//! The PRD feature-index markers (scaffolded by ticket 0017, written by ticket 0028).
//!
//! The markers are HTML comments, invisible when rendered, and delimit the
//! region of `sdlc/PRD.md` that the index writer rewrites whenever a feature is
//! created, renamed, deleted or changes status.

/// The marker that opens the PRD feature index region.
pub const FEATURE_INDEX_START: &str = "<!-- sdlc:feature-index:start -->";

/// The marker that closes the PRD feature index region.
pub const FEATURE_INDEX_END: &str = "<!-- sdlc:feature-index:end -->";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_index_markers_are_the_exact_contract_strings() {
        // The scaffold writes the region between these exact strings, and the
        // index writer of ticket 0028 rewrites from the same markers.
        assert_eq!(FEATURE_INDEX_START, "<!-- sdlc:feature-index:start -->");
        assert_eq!(FEATURE_INDEX_END, "<!-- sdlc:feature-index:end -->");
        assert_ne!(FEATURE_INDEX_START, FEATURE_INDEX_END);
    }
}
