//! Maps byte offsets to source locations.
//!
//! The [`SourceMap`] converts byte indices (from [`Span`](logos::Span)s on AST nodes)
//! back to human-readable `(line, column)` positions. It is used by every compilation
//! phase to produce error messages with source locations.

// Simple implementation. In this case only line starts for later look up
// An extended version could be used to print the source at that location and add little arrows to it
// Similar to Rust compiler

/// Maps byte offsets in source code to `(line, column)` positions.
///
/// Precomputes line start offsets at construction time and uses binary search
/// for lookups, so repeated queries are efficient.
pub struct SourceMap {
    _source: String,
    line_starts: Vec<usize>,
}

impl SourceMap {
    pub fn new(source: String) -> Self {
        let mut line_starts = vec![0]; // The first line always starts at index 0.
        // Find the start of each subsequent line.
        line_starts.extend(source.match_indices('\n').map(|(i, _)| i + 1));

        SourceMap {
            _source: source,
            line_starts,
        }
    }

    /// Converts a byte index into a `(line, column)` tuple.
    ///
    /// Both line and column numbers are 1-based.
    pub fn get_line_column(&self, byte_idx: usize) -> (usize, usize) {
        // Use binary search to find the line the byte_idx is on.
        let line_num = match self.line_starts.binary_search(&byte_idx) {
            Ok(line_idx) => line_idx + 1, // Exact match, byte is start of this line.
            Err(next_line_idx) => next_line_idx, // Not an exact match, belongs to the previous line.
        };

        // The column is the offset from the start of that line.
        let line_start_idx = self.line_starts[line_num - 1];
        let col_num = byte_idx - line_start_idx + 1;

        (line_num, col_num)
    }

    /// A convenience function to convert an entire Span.
    pub fn get_span_location(&self, span: &logos::Span) -> (usize, usize) {
        self.get_line_column(span.start)
    }
}
