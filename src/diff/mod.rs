//! Diff algorithm

use bytes::Bytes;
use thiserror::Error;

pub mod binary;
pub mod similar;

pub use binary::{BinaryDiffCodec, DiffOperation};

/// Errors that can occur during diff operations
#[derive(Debug, Error)]
pub enum DiffError {
    /// Invalid diff format
    #[error("Invalid diff format: {0}")]
    InvalidFormat(String),

    /// Diff computation failed
    #[error("Diff computation failed: {0}")]
    ComputationFailed(String),

    /// Patch application failed
    #[error("Patch application failed: {0}")]
    PatchFailed(String),
}

/// Trait for diff engines that can compute and apply binary diffs
pub trait DiffEngine: Send + Sync {
    /// Compute binary diff between old and new versions
    ///
    /// # Arguments
    /// * `old` - Previous version of the resource
    /// * `new` - Current version of the resource
    ///
    /// # Returns
    /// Binary diff that can be applied to transform `old` into `new`
    ///
    /// # Errors
    /// Returns [`DiffError`] if diff computation fails
    fn compute_diff(&self, old: &[u8], new: &[u8]) -> Result<Bytes, DiffError>;

    /// Apply binary diff to base content
    ///
    /// # Arguments
    /// * `base` - Base content to apply diff to
    /// * `diff` - Binary diff to apply
    ///
    /// # Returns
    /// Result of applying diff to base content
    ///
    /// # Errors
    /// Returns [`DiffError`] if patch application fails
    fn apply_diff(&self, base: &[u8], diff: &[u8]) -> Result<Bytes, DiffError>;

    /// Check if diff is worthwhile (provides sufficient compression)
    ///
    /// # Arguments
    /// * `original_size` - Size of original content
    /// * `diff_size` - Size of computed diff
    ///
    /// # Returns
    /// `true` if diff provides >20% savings, `false` otherwise
    fn is_diff_worthwhile(&self, original_size: usize, diff_size: usize) -> bool {
        diff_size < original_size * 80 / 100 // 20% savings
    }
}
