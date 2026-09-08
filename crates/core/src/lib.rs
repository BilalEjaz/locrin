pub mod baseline;
pub mod config;
pub mod edges;
pub mod entry;
pub mod finding;
pub mod imports;
pub mod index;
pub mod indexer;
pub mod lang;
pub mod parse;
pub mod project;
pub mod resolve;
pub mod symbols;
pub mod tree;
pub mod walk;

pub const ENGINE_NAME: &str = "locrin";

/// The in-source suppression marker. A finding whose line carries this text is
/// dropped by the rule runner, and the indexer records the lines that carry it so
/// suppression also works for findings on files the run did not parse.
pub const ALLOW_MARK: &str = "locrin:allow";

#[cfg(test)]
mod tests {
    #[test]
    fn engine_has_a_name() {
        assert_eq!(super::ENGINE_NAME, "locrin");
    }
}
