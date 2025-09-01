//! Diff implementation using the `similar` crate for computing diffs and BinaryDiffCodec for wire format

use super::{
    DiffEngine, DiffError,
    binary::{BinaryDiffCodec, DiffOperation},
};
use bytes::Bytes;
use similar::{Algorithm, ChangeTag, TextDiff};

/// Diff engine using the `similar` crate with line-based diffing
pub struct SimilarDiffEngine {
    /// Minimum compression ratio required (0.0 to 1.0, where 0.2 = 20% savings required)
    min_compression_ratio: f32,
}

impl SimilarDiffEngine {
    /// Create new diff engine
    pub fn new() -> Self {
        Self {
            min_compression_ratio: 0.2,
        }
    }

    /// Create new diff engine with custom compression ratio
    pub fn with_compression_ratio(min_compression_ratio: f32) -> Self {
        Self {
            min_compression_ratio: min_compression_ratio.clamp(0.0, 1.0),
        }
    }

    /// Convert bytes to string for text diffing
    fn to_string(data: &[u8]) -> String {
        String::from_utf8_lossy(data).into_owned()
    }
}

impl Default for SimilarDiffEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DiffEngine for SimilarDiffEngine {
    fn compute_diff(&self, old: &[u8], new: &[u8]) -> Result<Bytes, DiffError> {
        if old == new {
            // No changes - return empty operations list
            return BinaryDiffCodec::encode_diff(&[]);
        }

        let old_str = Self::to_string(old);
        let new_str = Self::to_string(new);

        let diff = TextDiff::configure()
            .algorithm(Algorithm::Myers)
            .diff_lines(&old_str, &new_str);

        let mut ops = Vec::new();

        for change in diff.iter_all_changes() {
            let text = change.value();
            let bytes = text.as_bytes();

            match change.tag() {
                ChangeTag::Equal => {
                    if !bytes.is_empty() {
                        ops.push(DiffOperation::Copy {
                            offset: 0,
                            length: bytes.len() as u32,
                        });
                    }
                }
                ChangeTag::Delete => {
                    if !bytes.is_empty() {
                        ops.push(DiffOperation::Delete {
                            length: bytes.len() as u32,
                        });
                    }
                }
                ChangeTag::Insert => {
                    if !bytes.is_empty() {
                        ops.push(DiffOperation::Insert(bytes.to_vec()));
                    }
                }
            }
        }

        BinaryDiffCodec::encode_diff(&ops)
    }

    fn apply_diff(&self, base: &[u8], diff: &[u8]) -> Result<Bytes, DiffError> {
        if diff.is_empty() {
            return Err(DiffError::PatchFailed("Empty diff".to_string()));
        }

        // Check for minimal diff (just END marker)
        if diff.len() == 1 && diff[0] == 0x04 {
            // DiffOp::End as u8
            return Ok(Bytes::copy_from_slice(base));
        }

        BinaryDiffCodec::apply_diff(base, diff)
    }

    fn is_diff_worthwhile(&self, original_size: usize, diff_size: usize) -> bool {
        if original_size == 0 {
            return false;
        }
        let compression_ratio = diff_size as f32 / original_size as f32;
        compression_ratio <= (1.0 - self.min_compression_ratio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_changes() {
        let engine = SimilarDiffEngine::new();
        let data = b"hello world";

        let diff = engine.compute_diff(data, data).unwrap();
        let result = engine.apply_diff(data, &diff).unwrap();

        assert_eq!(result.as_ref(), data);
        assert_eq!(diff.len(), 1); // Just the END marker
    }

    #[test]
    fn test_simple_change() {
        let engine = SimilarDiffEngine::new();
        let old = b"hello world";
        let new = b"hello universe";

        let diff = engine.compute_diff(old, new).unwrap();
        let result = engine.apply_diff(old, &diff).unwrap();

        assert_eq!(result.as_ref(), new);
    }

    #[test]
    fn test_diff_worthwhile() {
        let engine = SimilarDiffEngine::new();

        // Should be worthwhile (80% savings)
        assert!(engine.is_diff_worthwhile(1000, 200));

        // Should not be worthwhile (only 10% savings)
        assert!(!engine.is_diff_worthwhile(1000, 900));
    }
}
