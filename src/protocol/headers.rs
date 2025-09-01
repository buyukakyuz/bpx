//! DSP HTTP headers handling

/// DSP header constants
pub struct DspHeaders;

impl DspHeaders {
    /// Client session identifier
    pub const SESSION: &'static str = "X-DSP-Session";
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
    pub const CACHE_TTL: &'static str = "X-DSP-Cache-TTL";

    /// Get all DSP header names
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

    /// Check if a header name is a DSP header
    pub fn is_dsp_header(name: &str) -> bool {
        Self::all().contains(&name)
    }
}