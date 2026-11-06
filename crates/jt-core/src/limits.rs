use crate::{ParseError, ParseErrorKind};

/// Inspection limits. These bound accepted input/counts, not total process RSS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadLimits {
    pub max_input_bytes: u64,
    pub max_segments: u32,
    pub max_segment_output_bytes: u64,
    pub max_total_output_bytes: u64,
    pub max_lzma_dictionary_bytes: u64,
    pub max_elements: u32,
    pub max_xz_blocks: u32,
}
impl Default for ReadLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 512 * 1024 * 1024,
            max_segments: 1_000_000,
            max_segment_output_bytes: 256 * 1024 * 1024,
            max_total_output_bytes: 512 * 1024 * 1024,
            max_lzma_dictionary_bytes: 64 * 1024 * 1024,
            max_elements: 1_000_000,
            max_xz_blocks: 65_536,
        }
    }
}

impl ReadLimits {
    pub fn check_input_size(&self, size: u64, source_id: &str) -> Result<(), ParseError> {
        if size > self.max_input_bytes {
            return Err(ParseError::at(
                ParseErrorKind::LimitExceeded,
                "input_limit_exceeded",
                "input exceeds max_input_bytes",
                source_id,
                0,
                0,
            ));
        }
        Ok(())
    }
}
