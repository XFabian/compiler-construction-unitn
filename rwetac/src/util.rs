//! Small utility functions and traits used across the compiler.

/// Rounds `x` up to the nearest multiple of `n`.
///
/// Used during memory layout calculations to ensure proper alignment.
///
/// # Panics
///
/// Panics if `n` is not positive.
pub fn round_to_n(x: isize, n: isize) -> isize {
    if n <= 0 {
        panic!("n must be positive");
    }

    if x % n == 0 {
        x
    } else if x < 0 {
        x - n - (x % n)
    } else {
        x + n - (x % n)
    }
}

/// Extends the file name of a path with a given suffix, preserving the extension.
///
/// For example, `("path/to/file.txt", "_opt")` becomes `"path/to/file_opt.txt"`.
/// Used to generate debug output paths (e.g. the optimized WTAC dump).
pub fn extend_file_name(src: &std::path::Path, suffix: &str) -> Option<std::path::PathBuf> {
    // 1. Get the essential components of the path.
    let parent = src.parent().unwrap_or_else(|| std::path::Path::new(""));
    let stem = src.file_stem()?.to_str()?; // file_stem is the name without the extension.
    let extension = src.extension().and_then(|s| s.to_str()).unwrap_or("");

    // 2. Construct the new file name.
    let new_filename = if extension.is_empty() {
        format!("{}{}", stem, suffix)
    } else {
        format!("{}{}.{}", stem, suffix, extension)
    };

    // 3. Join the parent path with the new file name.
    Some(parent.join(new_filename))
}

/// A trait for merging two spans into one that covers both.
pub trait Merge {
    /// Returns a new span that covers the range from the earliest start
    /// to the latest end of `self` and `other`.
    fn merge(&self, other: &Self) -> Self;
}

impl Merge for logos::Span {
    fn merge(&self, other: &Self) -> Self {
        logos::Span {
            start: std::cmp::min(self.start, other.start),
            end: std::cmp::max(self.end, other.end),
        }
    }
}

/// Logs a separator line with a phase label. Used to visually divide
/// compilation phases in the tracing output.
#[macro_export]
macro_rules! log_separator {
    ($phase:expr) => {
        info!(phase = $phase, "{}", &"=".repeat(80));
    };
}
