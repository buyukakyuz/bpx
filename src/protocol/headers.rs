//! BPX HTTP headers handling

/// BPX header constants
pub struct BpxHeaders;

impl BpxHeaders {
    /// Client session identifier
    pub const SESSION: &'static str = "X-BPX-Session";
    /// Version client currently has
    pub const BASE_VERSION: &'static str = "X-Base-Version";
    /// Comma-separated diff formats client supports
    pub const ACCEPT_DIFF: &'static str = "Accept-Diff";
    /// Current version identifier
    pub const RESOURCE_VERSION: &'static str = "X-Resource-Version";
    /// Format of diff in body
    pub const DIFF_TYPE: &'static str = "X-Diff-Type";
    /// Size of full resource in bytes
    pub const ORIGINAL_SIZE: &'static str = "X-Original-Size";
    /// Size of diff in bytes
    pub const DIFF_SIZE: &'static str = "X-Diff-Size";
    /// How long client should cache this version (seconds)
    pub const CACHE_TTL: &'static str = "X-BPX-Cache-TTL";

    /// Get all BPX header names
    pub fn all() -> &'static [&'static str] {
        &[
            Self::SESSION,
            Self::BASE_VERSION,
            Self::ACCEPT_DIFF,
            Self::RESOURCE_VERSION,
            Self::DIFF_TYPE,
            Self::ORIGINAL_SIZE,
            Self::DIFF_SIZE,
            Self::CACHE_TTL,
        ]
    }

    /// Check if a header name is a BPX header
    pub fn is_bpx_header(name: &str) -> bool {
        Self::all().contains(&name)
    }
}
