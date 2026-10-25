//! The bounds every reader works inside (`specs/014` §3.1). A reader that reaches one stops, returns
//! what it has, and says so; it never allocates past a limit on the word of the input.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Bytes of input a reader is given.
    pub max_input: usize,
    /// Bytes of text one call returns.
    pub max_output: usize,
    /// Members read from or listed in one archive.
    pub max_members: usize,
    /// Total bytes an archive may expand to.
    pub max_expanded: usize,
    /// Archive-in-archive and object-in-object nesting.
    pub max_depth: usize,
    /// XML nodes or PDF objects visited.
    pub max_nodes: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_input: 64 << 20, max_output: 256 * 1024, max_members: 5_000, max_expanded: 256 << 20, max_depth: 3, max_nodes: 5_000_000 }
    }
}

/// An expanded size may not exceed this multiple of the compressed size.
pub const MAX_EXPANSION_RATIO: usize = 100;
