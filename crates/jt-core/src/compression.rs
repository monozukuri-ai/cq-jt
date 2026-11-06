//! Bounded outer codecs. XZ framing follows the public-domain XZ format 1.2.1.
use crate::{ParseError, ParseErrorKind, ReadLimits, SourceSpan};
use flate2::{Decompress, FlushDecompress, Status};
use std::io::{Cursor, Read};

pub(crate) fn failure(
    source: &SourceSpan,
    kind: ParseErrorKind,
    code: &'static str,
    message: impl Into<String>,
) -> ParseError {
    ParseError {
        kind,
        code,
        message: message.into(),
        source: Some(source.clone()),
    }
}

fn append(
    out: &mut Vec<u8>,
    bytes: &[u8],
    cap: u64,
    source: &SourceSpan,
) -> Result<(), ParseError> {
    if (out.len() as u64).saturating_add(bytes.len() as u64) > cap {
        return Err(failure(
            source,
            ParseErrorKind::LimitExceeded,
            "output_limit_exceeded",
            "decoded data exceeds segment or total output limit",
        ));
    }
    out.try_reserve(bytes.len()).map_err(|_| {
        failure(
            source,
            ParseErrorKind::LimitExceeded,
            "allocation_failed",
            "cannot allocate decoded buffer",
        )
    })?;
    out.extend_from_slice(bytes);
    Ok(())
}

pub(crate) fn copy(bytes: &[u8], cap: u64, source: &SourceSpan) -> Result<Vec<u8>, ParseError> {
    let mut out = Vec::new();
    append(&mut out, bytes, cap, source)?;
    Ok(out)
}

pub(crate) fn zlib(bytes: &[u8], cap: u64, source: &SourceSpan) -> Result<Vec<u8>, ParseError> {
    let mut codec = Decompress::new(true);
    let mut out = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let before = (codec.total_in(), codec.total_out());
        let state = codec
            .decompress(
                &bytes[before.0 as usize..],
                &mut buffer,
                FlushDecompress::None,
            )
            .map_err(|_| {
                failure(
                    source,
                    ParseErrorKind::InvalidCompression,
                    "invalid_zlib",
                    "invalid ZLIB stream or checksum",
                )
            })?;
        let produced = (codec.total_out() - before.1) as usize;
        append(&mut out, &buffer[..produced], cap, source)?;
        if state == Status::StreamEnd {
            if codec.total_in() != bytes.len() as u64 {
                return Err(failure(
                    source,
                    ParseErrorKind::InvalidCompression,
                    "compressed_trailing_data",
                    "bytes remain after the ZLIB stream",
                ));
            }
            return Ok(out);
        }
        if before == (codec.total_in(), codec.total_out()) {
            return Err(failure(
                source,
                ParseErrorKind::InvalidCompression,
                "truncated_zlib",
                "ZLIB stream has no complete terminator",
            ));
        }
    }
}

/// Validate every block's dictionary and declared chunk output before the decoder
/// can allocate it. This version of XzReader has no memory-limit constructor.
/// Restrict the initial profile to one LZMA2 filter per block; no concatenation.
fn xz_preflight(
    bytes: &[u8],
    cap: u64,
    limits: &ReadLimits,
    source: &SourceSpan,
) -> Result<u64, ParseError> {
    let invalid = || {
        failure(
            source,
            ParseErrorKind::InvalidCompression,
            "invalid_xz",
            "invalid or truncated XZ framing, index, sizes or checksum",
        )
    };
    let limit = |code, msg| failure(source, ParseErrorKind::LimitExceeded, code, msg);
    if bytes.len() < 32 || bytes.len() % 4 != 0 || !bytes.starts_with(b"\xfd7zXZ\0") {
        return Err(invalid());
    }
    let read32 = |at: usize| -> Result<u32, ParseError> {
        let value = bytes
            .get(at..at.checked_add(4).ok_or_else(invalid)?)
            .ok_or_else(invalid)?;
        Ok(u32::from_le_bytes(value.try_into().map_err(|_| invalid())?))
    };
    let footer = bytes.len() - 12;
    if &bytes[footer + 10..] != b"YZ"
        || bytes[6] != 0
        || bytes[6..8] != bytes[footer + 8..footer + 10]
        || read32(8)? != crc32fast::hash(&bytes[6..8])
        || read32(footer)? != crc32fast::hash(&bytes[footer + 4..footer + 10])
    {
        return Err(invalid());
    }
    let check_size = match bytes[7] {
        0 => 0,
        1 => 4,
        4 => 8,
        10 => 32,
        _ => {
            return Err(failure(
                source,
                ParseErrorKind::UnsupportedEncoding,
                "unsupported_xz_check",
                "unsupported XZ integrity check",
            ))
        }
    };
    let index_len = (u64::from(read32(footer + 4)?) + 1) * 4;
    let index = footer
        .checked_sub(usize::try_from(index_len).map_err(|_| invalid())?)
        .filter(|&v| v >= 12)
        .ok_or_else(invalid)?;
    if bytes[index] != 0
        || footer - index < 8
        || read32(footer - 4)? != crc32fast::hash(&bytes[index..footer - 4])
    {
        return Err(invalid());
    }
    let index_data = &bytes[..footer - 4];
    let mut at = index + 1;
    let records = vli(index_data, &mut at).ok_or_else(invalid)?;
    if records > u64::from(limits.max_xz_blocks) {
        return Err(limit(
            "xz_block_limit_exceeded",
            "XZ index exceeds max_xz_blocks",
        ));
    }
    if records > ((footer - 4 - at) / 2) as u64 {
        return Err(invalid());
    }
    let mut block = 12usize;
    let mut total = 0u64;
    for _ in 0..records {
        let unpadded = usize::try_from(vli(index_data, &mut at).ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
        let unpacked = vli(index_data, &mut at).ok_or_else(invalid)?;
        total = total.checked_add(unpacked).ok_or_else(invalid)?;
        if total > cap {
            return Err(limit(
                "output_limit_exceeded",
                "XZ index exceeds segment or total output limit",
            ));
        }
        let padded = unpadded.checked_add(3).ok_or_else(invalid)? & !3;
        let end = block
            .checked_add(padded)
            .filter(|&v| v <= index)
            .ok_or_else(invalid)?;
        let header_len = (usize::from(*bytes.get(block).ok_or_else(invalid)?) + 1) * 4;
        if header_len < 8 || unpadded < header_len + check_size || padded == 0 {
            return Err(invalid());
        }
        let header_end = block
            .checked_add(header_len)
            .filter(|&v| v <= end)
            .ok_or_else(invalid)?;
        if read32(header_end - 4)? != crc32fast::hash(&bytes[block..header_end - 4]) {
            return Err(invalid());
        }
        let flags = bytes[block + 1];
        if flags & 0x3f != 0 {
            return Err(failure(
                source,
                ParseErrorKind::UnsupportedEncoding,
                "unsupported_xz_filters",
                "only a single LZMA2 filter per XZ block is supported",
            ));
        }
        let mut h = block + 2;
        let header_data = &bytes[..header_end - 4];
        let packed_field = if flags & 0x40 != 0 {
            Some(vli(header_data, &mut h).ok_or_else(invalid)?)
        } else {
            None
        };
        let unpacked_field = if flags & 0x80 != 0 {
            Some(vli(header_data, &mut h).ok_or_else(invalid)?)
        } else {
            None
        };
        if vli(header_data, &mut h) != Some(0x21) || vli(header_data, &mut h) != Some(1) {
            return Err(failure(
                source,
                ParseErrorKind::UnsupportedEncoding,
                "unsupported_xz_filters",
                "XZ block must use the LZMA2 filter",
            ));
        }
        let prop = *header_data.get(h).ok_or_else(invalid)?;
        h += 1;
        if prop > 40
            || header_data
                .get(h..)
                .is_none_or(|padding| padding.iter().any(|&b| b != 0))
        {
            return Err(invalid());
        }
        let dict = if prop == 40 {
            u64::from(u32::MAX)
        } else {
            (2 | u64::from(prop & 1)) << (u32::from(prop / 2) + 11)
        };
        if dict > limits.max_lzma_dictionary_bytes {
            return Err(limit(
                "dictionary_limit_exceeded",
                "XZ LZMA2 dictionary exceeds max_lzma_dictionary_bytes",
            ));
        }
        let packed = unpadded - header_len - check_size;
        if packed_field.is_some_and(|v| v != packed as u64)
            || unpacked_field.is_some_and(|v| v != unpacked)
        {
            return Err(invalid());
        }
        let compressed_end = header_end + packed;
        if bytes[compressed_end..end - check_size]
            .iter()
            .any(|&b| b != 0)
        {
            return Err(invalid());
        }
        if lzma2_chunk_sizes(&bytes[header_end..compressed_end]) != Some(unpacked) {
            return Err(invalid());
        }
        block = end;
    }
    if block != index
        || index_data
            .get(at..)
            .is_none_or(|padding| padding.len() > 3 || padding.iter().any(|&b| b != 0))
    {
        return Err(invalid());
    }
    Ok(total)
}

fn vli(bytes: &[u8], at: &mut usize) -> Option<u64> {
    let mut value = 0;
    for shift in 0..9 {
        let b = *bytes.get(*at)?;
        *at += 1;
        if shift > 0 && b == 0 {
            return None;
        }
        value |= u64::from(b & 0x7f) << (7 * shift);
        if b & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// Walk chunk lengths without decoding to certify actual block offsets and the
/// total output size; a forged index must not hide an unchecked dictionary.
fn lzma2_chunk_sizes(bytes: &[u8]) -> Option<u64> {
    let mut at = 0;
    let mut total = 0u64;
    loop {
        let control = *bytes.get(at)?;
        at += 1;
        if control == 0 {
            return (at == bytes.len()).then_some(total);
        }
        let size = bytes.get(at..at.checked_add(2)?)?;
        let low = u64::from(u16::from_be_bytes(size.try_into().ok()?));
        at += 2;
        let packed;
        if control == 1 || control == 2 {
            packed = low + 1;
            total = total.checked_add(packed)?;
        } else if control >= 0x80 {
            let size = bytes.get(at..at.checked_add(2)?)?;
            packed = u64::from(u16::from_be_bytes(size.try_into().ok()?)) + 1;
            at += 2;
            total = total.checked_add((u64::from(control & 31) << 16) + low + 1)?;
            if control >= 0xc0 {
                at += 1;
            }
        } else {
            return None;
        }
        at = at.checked_add(usize::try_from(packed).ok()?)?;
        if at > bytes.len() {
            return None;
        }
    }
}

pub(crate) fn xz(
    bytes: &[u8],
    cap: u64,
    limits: &ReadLimits,
    source: &SourceSpan,
) -> Result<Vec<u8>, ParseError> {
    let expected = xz_preflight(bytes, cap, limits, source)?;
    let mut reader = lzma_rust2::XzReader::new(Cursor::new(bytes), false);
    let mut out = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let n = reader.read(&mut buffer).map_err(|_| {
            failure(
                source,
                ParseErrorKind::InvalidCompression,
                "invalid_xz",
                "XZ decoder rejected the stream or checksum",
            )
        })?;
        if n == 0 {
            break;
        }
        append(&mut out, &buffer[..n], cap, source)?;
    }
    if reader.inner().position() != bytes.len() as u64 || out.len() as u64 != expected {
        return Err(failure(
            source,
            ParseErrorKind::InvalidCompression,
            "invalid_xz",
            "XZ consumed/output sizes disagree with the validated index",
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    const XZ: &str = "fd377a585a000004e6d6b4460200210100000000372797d6e0018f001c5d00311bcb119b05063cd4b68b897ce1db83682e67b2bfb5b1bc33668800005afbf19bfe6c0bf80001389003000000e3d9cfe8b1c467fb020000000004595a";
    const ZLIB: &str = "789c4bca2fcd4b494d51c82f2d492d52282e294a4dcc4d1a151b30310025559d31";
    fn bytes(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .chunks_exact(2)
            .map(|s| u8::from_str_radix(std::str::from_utf8(s).unwrap(), 16).unwrap())
            .collect()
    }
    fn source() -> SourceSpan {
        SourceSpan {
            source_id: "test".into(),
            byte_domain: "file".into(),
            start_offset: 170,
            end_offset: 270,
        }
    }
    #[test]
    fn independently_encoded_zlib_and_xz_match_known_bytes() {
        let expected = b"bounded outer stream".repeat(20);
        assert_eq!(zlib(&bytes(ZLIB), 400, &source()).unwrap(), expected);
        assert_eq!(
            xz(&bytes(XZ), 400, &ReadLimits::default(), &source()).unwrap(),
            expected
        );
        assert_eq!(
            zlib(&bytes(ZLIB), 399, &source()).unwrap_err().code,
            "output_limit_exceeded"
        );
        assert_eq!(
            xz(&bytes(XZ), 399, &ReadLimits::default(), &source())
                .unwrap_err()
                .code,
            "output_limit_exceeded"
        );
    }
    #[test]
    fn all_codec_truncations_reject_and_mutations_do_not_panic() {
        let limits = ReadLimits {
            max_lzma_dictionary_bytes: 4096,
            ..ReadLimits::default()
        };
        for (encoded, is_xz) in [(bytes(ZLIB), false), (bytes(XZ), true)] {
            for end in 0..encoded.len() {
                let result = if is_xz {
                    xz(&encoded[..end], 4096, &limits, &source())
                } else {
                    zlib(&encoded[..end], 4096, &source())
                };
                assert!(result.is_err(), "accepted prefix {end}");
            }
            for at in 0..encoded.len() {
                for bit in 0..8 {
                    let mut changed = encoded.clone();
                    changed[at] ^= 1 << bit;
                    let _ = if is_xz {
                        xz(&changed, 4096, &limits, &source())
                    } else {
                        zlib(&changed, 4096, &source())
                    };
                }
            }
        }
    }
    #[test]
    fn canonical_vli_and_chunk_sizes_reject_overflow_and_bad_lengths() {
        assert_eq!(vli(&[0x80, 0], &mut 0), None);
        assert_eq!(vli(&[0xff; 10], &mut 0), None);
        assert_eq!(vli(&[0xff, 0x01], &mut 0), Some(255));
        assert_eq!(lzma2_chunk_sizes(&[0]), Some(0));
        assert_eq!(lzma2_chunk_sizes(&[0, 0]), None);
        assert_eq!(lzma2_chunk_sizes(&[1, 0, 1, 2, 3, 0]), Some(2));
        assert_eq!(lzma2_chunk_sizes(&[1, 0, 1, 2, 0]), None);
    }
}
