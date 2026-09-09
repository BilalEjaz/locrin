use std::collections::{HashMap, HashSet};

/// What the previous version of the repository said, for the rules whose answer
/// is a change rather than a state. `test-newly-skipped` is the first of them: a
/// test that was already skipped is a decision somebody made and lives with, and
/// a test skipped in this change is the finding.
///
/// The CLI fills this in; a rule only reads it. A file absent from a map has no
/// known previous version, so everything in it is new.
#[derive(Debug, Default, Clone)]
pub struct Previous {
    /// rel -> names of test cases that were skipped in the previous version of
    /// the file.
    pub skipped_tests: HashMap<String, HashSet<String>>,
}
