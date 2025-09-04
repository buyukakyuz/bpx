//! BPX wire format definitions

/// Binary diff operations
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffOp {
    /// Copy from old version
    Copy = 0x01,
    /// Insert new data
    Insert = 0x02,
    /// Delete/skip bytes from old version
    Delete = 0x03,
    /// End of diff stream
    End = 0x04,
}

impl DiffOp {
    /// Convert from byte value
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::Copy),
            0x02 => Some(Self::Insert),
            0x03 => Some(Self::Delete),
            0x04 => Some(Self::End),
            _ => None,
        }
    }

    /// Convert to byte value
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Get all valid operation codes
    pub fn all() -> &'static [DiffOp] {
        &[Self::Copy, Self::Insert, Self::Delete, Self::End]
    }

    /// Check if operation requires length parameter
    pub fn requires_length(self) -> bool {
        matches!(self, Self::Copy | Self::Insert | Self::Delete)
    }

    /// Check if operation requires data parameter
    pub fn requires_data(self) -> bool {
        matches!(self, Self::Insert)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_op_values() {
        // Test that enum values match expected wire format
        assert_eq!(DiffOp::Copy as u8, 0x01);
        assert_eq!(DiffOp::Insert as u8, 0x02);
        assert_eq!(DiffOp::Delete as u8, 0x03);
        assert_eq!(DiffOp::End as u8, 0x04);
    }

    #[test]
    fn test_diff_op_from_u8() {
        // Valid operations
        assert_eq!(DiffOp::from_u8(0x01), Some(DiffOp::Copy));
        assert_eq!(DiffOp::from_u8(0x02), Some(DiffOp::Insert));
        assert_eq!(DiffOp::from_u8(0x03), Some(DiffOp::Delete));
        assert_eq!(DiffOp::from_u8(0x04), Some(DiffOp::End));

        // Invalid operations
        assert_eq!(DiffOp::from_u8(0x00), None);
        assert_eq!(DiffOp::from_u8(0x05), None);
        assert_eq!(DiffOp::from_u8(0xFF), None);
    }

    #[test]
    fn test_diff_op_as_u8() {
        assert_eq!(DiffOp::Copy.as_u8(), 0x01);
        assert_eq!(DiffOp::Insert.as_u8(), 0x02);
        assert_eq!(DiffOp::Delete.as_u8(), 0x03);
        assert_eq!(DiffOp::End.as_u8(), 0x04);
    }

    #[test]
    fn test_diff_op_round_trip() {
        // Test that converting to u8 and back preserves value
        for op in DiffOp::all() {
            let byte = op.as_u8();
            let recovered = DiffOp::from_u8(byte);
            assert_eq!(recovered, Some(*op));
        }
    }

    #[test]
    fn test_all_operations() {
        let all_ops = DiffOp::all();
        assert_eq!(all_ops.len(), 4);
        assert!(all_ops.contains(&DiffOp::Copy));
        assert!(all_ops.contains(&DiffOp::Insert));
        assert!(all_ops.contains(&DiffOp::Delete));
        assert!(all_ops.contains(&DiffOp::End));
    }

    #[test]
    fn test_requires_length() {
        assert!(DiffOp::Copy.requires_length());
        assert!(DiffOp::Insert.requires_length());
        assert!(DiffOp::Delete.requires_length());
        assert!(!DiffOp::End.requires_length());
    }

    #[test]
    fn test_requires_data() {
        assert!(!DiffOp::Copy.requires_data());
        assert!(DiffOp::Insert.requires_data());
        assert!(!DiffOp::Delete.requires_data());
        assert!(!DiffOp::End.requires_data());
    }

    #[test]
    fn test_wire_format_constants() {
        // Ensure wire format constants haven't changed accidentally
        const EXPECTED_COPY: u8 = 0x01;
        const EXPECTED_INSERT: u8 = 0x02;
        const EXPECTED_DELETE: u8 = 0x03;
        const EXPECTED_END: u8 = 0x04;

        assert_eq!(DiffOp::Copy as u8, EXPECTED_COPY);
        assert_eq!(DiffOp::Insert as u8, EXPECTED_INSERT);
        assert_eq!(DiffOp::Delete as u8, EXPECTED_DELETE);
        assert_eq!(DiffOp::End as u8, EXPECTED_END);
    }

    #[test]
    fn test_operation_semantics() {
        // Test the logical meaning of operations
        assert_eq!(
            DiffOp::Copy.requires_length() && !DiffOp::Copy.requires_data(),
            true
        );
        assert_eq!(
            DiffOp::Insert.requires_length() && DiffOp::Insert.requires_data(),
            true
        );
        assert_eq!(
            DiffOp::Delete.requires_length() && !DiffOp::Delete.requires_data(),
            true
        );
        assert_eq!(
            !DiffOp::End.requires_length() && !DiffOp::End.requires_data(),
            true
        );
    }
}
