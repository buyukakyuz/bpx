//! Binary diff format
//!
//! Wire Format:
//! ```text
//! +--------+--------+----------------+
//! | Op(1B) | Len(3B)| Data           |
//! +--------+--------+----------------+
//! ```
//!
//! Operations:
//! - 0x01: COPY(offset: u32, length: u24) - copy from old version
//! - 0x02: INSERT(length: u24, data: [u8]) - insert new data  
//! - 0x03: DELETE(length: u24) - skip bytes from old version
//! - 0x04: END - end of diff stream
//!
//! # Example
//! ```
//! use dsp::diff::{BinaryDiffCodec, DiffOperation};
//!
//! let operations = vec![
//!     DiffOperation::Copy { offset: 0, length: 9 },
//!     DiffOperation::Delete { length: 3 },
//!     DiffOperation::Insert(b"Robert".to_vec()),
//!     DiffOperation::Copy { offset: 0, length: 2 },
//! ];
//!
//! let encoded = BinaryDiffCodec::encode_diff(&operations).unwrap();
//! let base = br#"{"name":"Bob"}"#;
//! let result = BinaryDiffCodec::apply_diff(base, &encoded).unwrap();
//! assert_eq!(result.as_ref(), br#"{"name":"Robert"}"#);
//! ```

use super::DiffError;
use crate::protocol::wire::DiffOp;
use bytes::{Buf, BufMut, Bytes, BytesMut};

/// Diff operation with data
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffOperation {
    /// Copy bytes from old version at specific offset
    Copy {
        /// Offset in the original content
        offset: u32,
        /// Number of bytes to copy
        length: u32,
    },
    /// Insert new data
    Insert(Vec<u8>),
    /// Delete/skip bytes from old version  
    Delete {
        /// Number of bytes to skip/delete
        length: u32,
    },
}

/// Binary diff encoder/decoder
pub struct BinaryDiffCodec;

impl BinaryDiffCodec {
    /// Encode diff operations to binary format
    ///
    /// # Arguments
    /// * `operations` - List of diff operations to encode
    ///
    /// # Returns
    /// Binary diff data following DSP wire format
    pub fn encode_diff(operations: &[DiffOperation]) -> Result<Bytes, DiffError> {
        let mut buf = BytesMut::new();

        for op in operations {
            match op {
                DiffOperation::Copy { offset: _, length } => {
                    // Copy format: [op(1B), length(3B), offset(4B)]
                    buf.put_u8(DiffOp::Copy as u8);
                    if *length > 0xFFFFFF {
                        return Err(DiffError::InvalidFormat(
                            "Copy length too large (max 24-bit)".to_string(),
                        ));
                    }
                    buf.put_uint(*length as u64, 3);
                    // we don't use offset
                    // since we're doing sequential copying. Offset would be used
                    // for more sophisticated diff algorithms - will try Myer's soon.
                }
                DiffOperation::Insert(data) => {
                    // Insert format: [op(1B), length(3B), data...]
                    buf.put_u8(DiffOp::Insert as u8);
                    if data.len() > 0xFFFFFF {
                        return Err(DiffError::InvalidFormat(
                            "Insert data too large (max 24-bit length)".to_string(),
                        ));
                    }
                    buf.put_uint(data.len() as u64, 3);
                    buf.put_slice(data);
                }
                DiffOperation::Delete { length } => {
                    // Delete format: [op(1B), length(3B)]
                    buf.put_u8(DiffOp::Delete as u8);
                    if *length > 0xFFFFFF {
                        return Err(DiffError::InvalidFormat(
                            "Delete length too large (max 24-bit)".to_string(),
                        ));
                    }
                    buf.put_uint(*length as u64, 3);
                }
            }
        }

        buf.put_u8(DiffOp::End as u8);
        Ok(buf.freeze())
    }

    /// Decode binary diff data to operations
    ///
    /// # Arguments  
    /// * `diff_data` - Binary diff data following DSP wire format
    ///
    /// # Returns
    /// List of decoded diff operations
    pub fn decode_diff(diff_data: &[u8]) -> Result<Vec<DiffOperation>, DiffError> {
        let mut operations = Vec::new();
        let mut cursor = diff_data;

        while !cursor.is_empty() {
            let op_byte = cursor.get_u8();
            let op = DiffOp::from_u8(op_byte).ok_or_else(|| {
                DiffError::InvalidFormat(format!("Unknown operation: 0x{:02x}", op_byte))
            })?;

            match op {
                DiffOp::Copy => {
                    if cursor.remaining() < 3 {
                        return Err(DiffError::InvalidFormat(
                            "Insufficient data for Copy operation length".to_string(),
                        ));
                    }
                    let length = cursor.get_uint(3) as u32;
                    // offset is implicitly the current position
                    operations.push(DiffOperation::Copy { offset: 0, length });
                }
                DiffOp::Insert => {
                    if cursor.remaining() < 3 {
                        return Err(DiffError::InvalidFormat(
                            "Insufficient data for Insert operation length".to_string(),
                        ));
                    }
                    let length = cursor.get_uint(3) as usize;
                    if cursor.remaining() < length {
                        return Err(DiffError::InvalidFormat(
                            "Insufficient data for Insert operation payload".to_string(),
                        ));
                    }
                    let data = cursor[..length].to_vec();
                    cursor.advance(length);
                    operations.push(DiffOperation::Insert(data));
                }
                DiffOp::Delete => {
                    if cursor.remaining() < 3 {
                        return Err(DiffError::InvalidFormat(
                            "Insufficient data for Delete operation length".to_string(),
                        ));
                    }
                    let length = cursor.get_uint(3) as u32;
                    operations.push(DiffOperation::Delete { length });
                }
                DiffOp::End => {
                    break;
                }
            }
        }

        Ok(operations)
    }

    /// Apply diff operations to base content
    ///
    /// # Arguments
    /// * `base` - Original content to apply diff to
    /// * `operations` - Diff operations to apply
    ///
    /// # Returns
    /// Result of applying diff operations
    pub fn apply_operations(base: &[u8], operations: &[DiffOperation]) -> Result<Bytes, DiffError> {
        let mut result = BytesMut::new();
        let mut base_pos = 0;

        for op in operations {
            match op {
                DiffOperation::Copy { offset: _, length } => {
                    let end_pos = base_pos + *length as usize;
                    if end_pos > base.len() {
                        return Err(DiffError::PatchFailed(
                            "Copy operation exceeds base content length".to_string(),
                        ));
                    }
                    result.put_slice(&base[base_pos..end_pos]);
                    base_pos = end_pos;
                }
                DiffOperation::Insert(data) => {
                    result.put_slice(data);
                    // base_pos stays the same - we're inserting new content
                }
                DiffOperation::Delete { length } => {
                    base_pos += *length as usize;
                    if base_pos > base.len() {
                        return Err(DiffError::PatchFailed(
                            "Delete operation exceeds base content length".to_string(),
                        ));
                    }
                    // Skip deleted bytes - don't copy to result
                }
            }
        }

        Ok(result.freeze())
    }

    /// Convenience method to apply binary diff to base content
    ///
    /// # Arguments
    /// * `base` - Original content
    /// * `diff_data` - Binary diff data
    ///
    /// # Returns
    /// Reconstructed content after applying diff
    pub fn apply_diff(base: &[u8], diff_data: &[u8]) -> Result<Bytes, DiffError> {
        let operations = Self::decode_diff(diff_data)?;
        Self::apply_operations(base, &operations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::wire::DiffOp;

    #[test]
    fn test_encode_decode_copy_operation() {
        let operations = vec![DiffOperation::Copy {
            offset: 0,
            length: 5,
        }];

        let encoded = BinaryDiffCodec::encode_diff(&operations).unwrap();
        let decoded = BinaryDiffCodec::decode_diff(&encoded).unwrap();

        assert_eq!(operations, decoded);

        // Check wire format: [COPY(1B), length(3B), END(1B)]
        assert_eq!(encoded.len(), 5); // 1 + 3 + 1
        assert_eq!(encoded[0], DiffOp::Copy as u8);
        assert_eq!(encoded[4], DiffOp::End as u8);
    }

    #[test]
    fn test_encode_decode_insert_operation() {
        let data = b"hello world".to_vec();
        let operations = vec![DiffOperation::Insert(data.clone())];

        let encoded = BinaryDiffCodec::encode_diff(&operations).unwrap();
        let decoded = BinaryDiffCodec::decode_diff(&encoded).unwrap();

        assert_eq!(operations, decoded);

        // Check wire format: [INSERT(1B), length(3B), data(11B), END(1B)]
        assert_eq!(encoded.len(), 1 + 3 + 11 + 1);
        assert_eq!(encoded[0], DiffOp::Insert as u8);
        assert_eq!(encoded[15], DiffOp::End as u8);

        // Check data is correctly encoded
        let encoded_data = &encoded[4..15];
        assert_eq!(encoded_data, data.as_slice());
    }

    #[test]
    fn test_encode_decode_delete_operation() {
        let operations = vec![DiffOperation::Delete { length: 3 }];

        let encoded = BinaryDiffCodec::encode_diff(&operations).unwrap();
        let decoded = BinaryDiffCodec::decode_diff(&encoded).unwrap();

        assert_eq!(operations, decoded);

        // Check wire format: [DELETE(1B), length(3B), END(1B)]
        assert_eq!(encoded.len(), 5);
        assert_eq!(encoded[0], DiffOp::Delete as u8);
        assert_eq!(encoded[4], DiffOp::End as u8);
    }

    #[test]
    fn test_encode_decode_complex_sequence() {
        let operations = vec![
            DiffOperation::Copy {
                offset: 0,
                length: 7,
            },
            DiffOperation::Delete { length: 3 },
            DiffOperation::Insert(b"Robert".to_vec()),
            DiffOperation::Copy {
                offset: 0,
                length: 2,
            },
        ];

        let encoded = BinaryDiffCodec::encode_diff(&operations).unwrap();
        let decoded = BinaryDiffCodec::decode_diff(&encoded).unwrap();

        assert_eq!(operations, decoded);
    }

    #[test]
    fn test_apply_operations_copy() {
        let base = b"Hello, World!";
        let operations = vec![DiffOperation::Copy {
            offset: 0,
            length: 5,
        }];

        let result = BinaryDiffCodec::apply_operations(base, &operations).unwrap();
        assert_eq!(result.as_ref(), b"Hello");
    }

    #[test]
    fn test_apply_operations_insert() {
        let base = b"Hello";
        let operations = vec![
            DiffOperation::Copy {
                offset: 0,
                length: 5,
            },
            DiffOperation::Insert(b", World!".to_vec()),
        ];

        let result = BinaryDiffCodec::apply_operations(base, &operations).unwrap();
        assert_eq!(result.as_ref(), b"Hello, World!");
    }

    #[test]
    fn test_apply_operations_delete() {
        let base = b"Hello, cruel World!";
        let operations = vec![
            DiffOperation::Copy {
                offset: 0,
                length: 7,
            }, // "Hello, "
            DiffOperation::Delete { length: 6 }, // skip "cruel "
            DiffOperation::Copy {
                offset: 0,
                length: 6,
            }, // "World!"
        ];

        let result = BinaryDiffCodec::apply_operations(base, &operations).unwrap();
        assert_eq!(result.as_ref(), b"Hello, World!");
    }

    #[test]
    fn test_json_name_change_example() {
        // {"name":"Bob"} -> {"name":"Robert"}
        let base = br#"{"name":"Bob"}"#;
        let operations = vec![
            DiffOperation::Copy {
                offset: 0,
                length: 9,
            }, // `{"name":"`
            DiffOperation::Delete { length: 3 }, // delete "Bob"
            DiffOperation::Insert(b"Robert".to_vec()), // insert "Robert"
            DiffOperation::Copy {
                offset: 0,
                length: 2,
            }, // `"}"`
        ];

        let result = BinaryDiffCodec::apply_operations(base, &operations).unwrap();
        assert_eq!(result.as_ref(), br#"{"name":"Robert"}"#);
    }

    #[test]
    fn test_roundtrip_encode_apply_diff() {
        let base = b"The quick brown fox";
        let operations = vec![
            DiffOperation::Copy {
                offset: 0,
                length: 10,
            }, // "The quick "
            DiffOperation::Delete { length: 5 }, // delete "brown"
            DiffOperation::Insert(b"red".to_vec()), // insert "red"
            DiffOperation::Copy {
                offset: 0,
                length: 4,
            }, // " fox"
        ];

        let encoded = BinaryDiffCodec::encode_diff(&operations).unwrap();
        let result = BinaryDiffCodec::apply_diff(base, &encoded).unwrap();

        assert_eq!(result.as_ref(), b"The quick red fox");
    }

    #[test]
    fn test_empty_operations() {
        let operations = vec![];
        let encoded = BinaryDiffCodec::encode_diff(&operations).unwrap();
        let decoded = BinaryDiffCodec::decode_diff(&encoded).unwrap();

        assert_eq!(operations, decoded);
        assert_eq!(encoded.len(), 1);
        assert_eq!(encoded[0], DiffOp::End as u8);
    }

    #[test]
    fn test_apply_empty_diff() {
        let base = b"unchanged";
        let operations = vec![];
        let result = BinaryDiffCodec::apply_operations(base, &operations).unwrap();

        assert_eq!(result.len(), 0); // Empty result since no operations
    }

    #[test]
    fn test_large_length_error() {
        // Test that lengths > 24-bit (0xFFFFFF) are rejected
        let operations = vec![DiffOperation::Copy {
            offset: 0,
            length: 0x1000000,
        }]; // > 24-bit

        let result = BinaryDiffCodec::encode_diff(&operations);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Copy length too large")
        );
    }

    #[test]
    fn test_large_insert_data_error() {
        // Test that insert data > 24-bit length is rejected
        let large_data = vec![0u8; 0x1000000]; // > 24-bit length
        let operations = vec![DiffOperation::Insert(large_data)];

        let result = BinaryDiffCodec::encode_diff(&operations);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Insert data too large")
        );
    }

    #[test]
    fn test_decode_invalid_operation() {
        // Test decoding with invalid operation code
        let invalid_data = vec![0xFF, 0x00, 0x00, 0x01]; // Invalid op code

        let result = BinaryDiffCodec::decode_diff(&invalid_data);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Unknown operation: 0xff")
        );
    }

    #[test]
    fn test_decode_truncated_data() {
        // Test decoding with insufficient data
        let truncated_data = vec![DiffOp::Copy as u8, 0x00]; // Missing length bytes

        let result = BinaryDiffCodec::decode_diff(&truncated_data);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Insufficient data")
        );
    }

    #[test]
    fn test_apply_copy_beyond_base() {
        let base = b"short";
        let operations = vec![DiffOperation::Copy {
            offset: 0,
            length: 100,
        }]; // Beyond base length

        let result = BinaryDiffCodec::apply_operations(base, &operations);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("exceeds base content length")
        );
    }

    #[test]
    fn test_apply_delete_beyond_base() {
        let base = b"short";
        let operations = vec![DiffOperation::Delete { length: 100 }]; // Beyond base length

        let result = BinaryDiffCodec::apply_operations(base, &operations);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("exceeds base content length")
        );
    }

    #[test]
    fn test_wire_format_compliance() {
        // Test specific wire format as per specification
        let operations = vec![DiffOperation::Insert(b"test".to_vec())];
        let encoded = BinaryDiffCodec::encode_diff(&operations).unwrap();

        // Expected format: [INSERT(0x02), length(0x000004), data("test"), END(0x04)]
        let expected = vec![
            0x02, // INSERT
            0x00, 0x00, 0x04, // length = 4 (24-bit big-endian)
            b't', b'e', b's', b't', // data
            0x04, // END
        ];

        assert_eq!(encoded.as_ref(), expected.as_slice());
    }

    #[test]
    fn test_max_24bit_values() {
        // Test maximum 24-bit values work correctly
        let max_24bit = 0xFFFFFF;
        let operations = vec![
            DiffOperation::Copy {
                offset: 0,
                length: max_24bit,
            },
            DiffOperation::Delete { length: max_24bit },
        ];

        let encoded = BinaryDiffCodec::encode_diff(&operations).unwrap();
        let decoded = BinaryDiffCodec::decode_diff(&encoded).unwrap();

        assert_eq!(operations, decoded);
    }
}
