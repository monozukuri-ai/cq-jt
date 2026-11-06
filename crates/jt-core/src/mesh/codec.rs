//! JT 9.5 Rev A sections 8.1.2, 8.2 and Appendix C. Copyright 2010 Siemens.
use crate::{compression::failure, ByteOrder, ParseError, ParseErrorKind as K, SourceSpan};

pub(super) struct Cursor<'a> {
    pub bytes: &'a [u8],
    pub at: usize,
    pub order: ByteOrder,
    pub source: SourceSpan,
    pub modern: bool,
    pub array_float_hash: bool,
    pub externally_compressed: bool,
}
impl<'a> Cursor<'a> {
    pub fn error(&self, kind: K, code: &'static str, message: impl Into<String>) -> ParseError {
        let mut span = self.source.clone();
        span.start_offset += self.at as u64;
        span.end_offset = span.start_offset;
        failure(&span, kind, code, message)
    }
    pub fn invalid(&self, message: impl Into<String>) -> ParseError {
        self.error(K::InvalidGeometry, "invalid_mesh_data", message)
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], ParseError> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&e| e <= self.bytes.len())
            .ok_or_else(|| self.invalid("mesh data extends beyond its element"))?;
        let bytes = &self.bytes[self.at..end];
        self.at = end;
        Ok(bytes)
    }
    pub fn u8(&mut self) -> Result<u8, ParseError> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16, ParseError> {
        let b: [u8; 2] = self.take(2)?.try_into().unwrap();
        Ok(match self.order {
            ByteOrder::Little => u16::from_le_bytes(b),
            ByteOrder::Big => u16::from_be_bytes(b),
        })
    }
    pub fn u32(&mut self) -> Result<u32, ParseError> {
        let b: [u8; 4] = self.take(4)?.try_into().unwrap();
        Ok(match self.order {
            ByteOrder::Little => u32::from_le_bytes(b),
            ByteOrder::Big => u32::from_be_bytes(b),
        })
    }
    pub fn i32(&mut self) -> Result<i32, ParseError> {
        Ok(self.u32()? as i32)
    }
    pub fn u64(&mut self) -> Result<u64, ParseError> {
        let a = self.u32()? as u64;
        let b = self.u32()? as u64;
        Ok(match self.order {
            ByteOrder::Little => a | (b << 32),
            ByteOrder::Big => (a << 32) | b,
        })
    }
    pub fn f32(&mut self) -> Result<f64, ParseError> {
        let v = f32::from_bits(self.u32()?) as f64;
        if v.is_finite() {
            Ok(v)
        } else {
            Err(self.invalid("non-finite mesh float"))
        }
    }
    pub fn count(&mut self, cap: u64) -> Result<usize, ParseError> {
        let n = self.i32()?;
        if n < 0 {
            return Err(self.invalid("negative mesh count"));
        }
        if n as u64 > cap {
            return Err(self.error(
                K::LimitExceeded,
                "mesh_limit_exceeded",
                "mesh count exceeds its configured limit",
            ));
        }
        Ok(n as usize)
    }
    pub fn version(&mut self, expected: u16) -> Result<(), ParseError> {
        let v = if self.modern {
            u16::from(self.u8()?)
        } else {
            self.u16()?
        };
        if v == expected {
            Ok(())
        } else {
            Err(self.error(
                K::UnsupportedEncoding,
                "unsupported_mesh_version",
                format!("mesh local version {v}, expected {expected}"),
            ))
        }
    }
    pub fn done(&self) -> Result<(), ParseError> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(self.error(
                K::UnsupportedEncoding,
                "unparsed_mesh_data",
                format!(
                    "{} unrecognized trailing mesh bytes at {}: {:02x?}",
                    self.bytes.len() - self.at,
                    self.at,
                    &self.bytes[self.at..self.bytes.len().min(self.at + 16)]
                ),
            ))
        }
    }
}

#[derive(Debug)]
pub(super) struct Budget {
    pub remaining: u64,
    pub packets: u64,
    pub depth: u32,
    pub deepest: u32,
}
impl Budget {
    pub fn charge(&mut self, r: &Cursor<'_>, n: usize) -> Result<(), ParseError> {
        self.remaining = self.remaining.checked_sub(n as u64).ok_or_else(|| {
            r.error(
                K::LimitExceeded,
                "codec_value_limit",
                "compressed integer allocation budget exceeded",
            )
        })?;
        self.packets = self.packets.checked_sub(1).ok_or_else(|| {
            r.error(
                K::LimitExceeded,
                "codec_packet_limit",
                "compressed packet budget exceeded",
            )
        })?;
        Ok(())
    }
}

pub(super) struct Bits<'a> {
    bytes: &'a [u8],
    pub at: usize,
    limit: usize,
}
impl<'a> Bits<'a> {
    pub fn new(bytes: &'a [u8], limit: usize) -> Self {
        Self {
            bytes,
            at: 0,
            limit,
        }
    }
    pub fn u(&mut self, n: u32) -> Result<u32, &'static str> {
        if n > 32
            || self
                .at
                .checked_add(n as usize)
                .is_none_or(|v| v > self.limit)
        {
            return Err("truncated or invalid bit field");
        }
        let mut v = 0;
        for _ in 0..n {
            v = (v << 1) | u32::from((self.bytes[self.at / 8] >> (7 - self.at % 8)) & 1);
            self.at += 1;
        }
        Ok(v)
    }
    pub fn i(&mut self, n: u32) -> Result<i32, &'static str> {
        let v = self.u(n)?;
        Ok(if n == 0 {
            0
        } else {
            ((v as i32) << (32 - n)) >> (32 - n)
        })
    }
    fn arithmetic_bit(&mut self) -> Result<u32, &'static str> {
        // Arithmetic termination permits zero extension of at most one register.
        if self.at >= self.limit {
            if self.at >= self.limit + 16 {
                return Err("arithmetic termination exceeds 16 padding bits");
            }
            self.at += 1;
            Ok(0)
        } else {
            self.u(1)
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Predictor {
    None,
    Lag1,
}
pub(super) fn vector(
    r: &mut Cursor<'_>,
    b: &mut Budget,
    p: Predictor,
) -> Result<Vec<i32>, ParseError> {
    let mut out = packet(r, b, 0)?;
    if matches!(p, Predictor::Lag1) {
        // First four are stored literally (Appendix C).
        for i in 4..out.len() {
            out[i] = out[i].wrapping_add(out[i - 1]);
        }
    }
    Ok(out)
}
fn packet(r: &mut Cursor<'_>, b: &mut Budget, depth: u32) -> Result<Vec<i32>, ParseError> {
    b.deepest = b.deepest.max(depth);
    if depth > b.depth {
        return Err(r.error(
            K::LimitExceeded,
            "codec_recursion_limit",
            "Int32CDP2 recursion limit exceeded",
        ));
    }
    let n = r.count(b.remaining)?;
    b.charge(r, n)?;
    if n == 0 {
        return Ok(Vec::new());
    }
    let codec = r.u8()?;
    if codec == 4 {
        let chop = r.u8()?;
        if chop == 0 && !r.modern {
            let out = packet(r, b, depth + 1)?;
            if out.len() != n {
                return Err(r.invalid("chopper count mismatch"));
            }
            return Ok(out);
        }
        let bias = r.i32()?;
        let span = r.u8()?;
        if span > 32 || chop > span || (r.modern && (chop == 0 || chop == 32)) {
            return Err(r.invalid("invalid chopper widths"));
        }
        let high = packet(r, b, depth + 1)?;
        let mut low = packet(r, b, depth + 1)?;
        if high.len() != n || low.len() != n {
            return Err(r.invalid("chopper child count mismatch"));
        }
        let shift = span - chop;
        let mask = if shift == 32 {
            u32::MAX
        } else {
            (1u32 << shift) - 1
        };
        for (h, l) in high.into_iter().zip(&mut low) {
            if (*l as u32) & !mask != 0 {
                return Err(r.invalid("chopper low field exceeds width"));
            }
            *l = (((*l as u32) | ((h as u32) << shift)) as i32).wrapping_add(bias);
        }
        return Ok(low);
    }
    if codec == 5 && r.modern {
        let values = packet(r, b, depth + 1)?;
        let offsets = packet(r, b, depth + 1)?;
        if offsets.len() != n {
            return Err(r.invalid("move-to-front output count mismatch"));
        }
        let mut window = Vec::<i32>::new();
        let mut at = 0usize;
        let mut output = Vec::with_capacity(n);
        for offset in offsets {
            let value = if offset == -1 {
                let value = *values
                    .get(at)
                    .ok_or_else(|| r.invalid("move-to-front escape exceeds values"))?;
                at += 1;
                value
            } else {
                if offset < 0 || offset as usize >= window.len() {
                    return Err(r.invalid("move-to-front window offset out of range"));
                }
                window.remove(offset as usize)
            };
            window.insert(0, value);
            window.truncate(16);
            output.push(value);
        }
        if at != values.len() {
            return Err(r.invalid("unused move-to-front escape values"));
        }
        return Ok(output);
    }
    if codec == 0 {
        // Legacy readers and actual 9.5 files store the following null length in bytes.
        let len = r.count(u64::MAX)?;
        if len
            != n.checked_mul(4)
                .ok_or_else(|| r.invalid("null packet size overflow"))?
        {
            return Err(r.invalid("null packet length/count mismatch"));
        }
        r.bytes
            .get(r.at..r.at + len)
            .ok_or_else(|| r.invalid("truncated null packet"))?;
        let mut out = Vec::new();
        out.try_reserve_exact(n).map_err(|_| {
            r.error(
                K::LimitExceeded,
                "allocation_failed",
                "cannot allocate integer packet",
            )
        })?;
        for _ in 0..n {
            out.push(r.i32()?);
        }
        return Ok(out);
    }
    if codec != 1 && codec != 3 {
        return Err(r.error(
            K::UnsupportedEncoding,
            "unsupported_int32_codec",
            format!("unsupported Int32CDP2 codec {codec}"),
        ));
    }
    let len = r.count((r.bytes.len() - r.at) as u64 * 8)?;
    let words = len.div_ceil(32);
    let raw = r.take(words * 4)?;
    let mut text = Vec::new();
    text.try_reserve_exact(raw.len()).map_err(|_| {
        r.error(
            K::LimitExceeded,
            "allocation_failed",
            "cannot allocate code text",
        )
    })?;
    for word in raw.chunks_exact(4) {
        if r.order == ByteOrder::Little {
            text.extend(word.iter().rev());
        } else {
            text.extend_from_slice(word);
        }
    }
    let mut bits = Bits::new(&text, len);
    let result = if codec == 1 {
        bitlength(&mut bits, n, r.modern)
    } else {
        let start = r.at;
        let mut ctxbits = Bits::new(&r.bytes[start..], (r.bytes.len() - start) * 8);
        let context = context(&mut ctxbits, b.remaining, r.modern).map_err(|e| {
            r.error(
                if e == "arithmetic context budget exceeded" {
                    K::LimitExceeded
                } else {
                    K::InvalidGeometry
                },
                "invalid_arithmetic_context",
                e,
            )
        })?;
        b.remaining -= context.len() as u64;
        let size = ctxbits.at.div_ceil(8);
        r.take(size)?;
        let oob = if !r.modern || context.iter().any(|e| e.symbol == -2) {
            if r.modern && r.externally_compressed {
                let n = r.count(b.remaining)?;
                b.charge(r, n)?;
                if n.checked_mul(4)
                    .is_none_or(|size| size > r.bytes.len() - r.at)
                {
                    return Err(r.invalid("truncated raw arithmetic out-of-band array"));
                }
                (0..n).map(|_| r.i32()).collect::<Result<Vec<_>, _>>()?
            } else {
                packet(r, b, depth + 1)?
            }
        } else {
            Vec::new()
        };
        if len == 0 && oob.len() == n {
            Ok(oob)
        } else {
            arithmetic(&mut bits, n, &context, &oob)
        }
    };
    result.map_err(|e| r.invalid(e))
}
fn nibble(bits: &mut Bits<'_>) -> Result<i32, &'static str> {
    let mut value = 0u32;
    for i in 0..8 {
        value |= bits.u(4)? << (i * 4);
        let more = bits.u(1)?;
        if more == 0 {
            let width = (i + 1) * 4;
            return Ok(if width == 32 {
                value as i32
            } else {
                ((value as i32) << (32 - width)) >> (32 - width)
            });
        }
    }
    Err("nibbler integer exceeds 32 bits")
}
fn bitlength(bits: &mut Bits<'_>, n: usize, modern: bool) -> Result<Vec<i32>, &'static str> {
    let mut out = Vec::new();
    out.try_reserve_exact(n)
        .map_err(|_| "integer allocation failed")?;
    if bits.u(1)? == 0 {
        let (min, max) = if modern {
            (nibble(bits)?, nibble(bits)?)
        } else {
            let a = bits.u(6)?;
            let z = bits.u(6)?;
            (bits.i(a)?, bits.i(z)?)
        };
        if max < min {
            return Err("invalid fixed-width interval");
        }
        let width = 32 - ((max as i64 - min as i64) as u32).leading_zeros();
        for _ in 0..n {
            let v = (bits.u(width)? as i64) + (min as i64);
            if v > max as i64 {
                return Err("fixed-width symbol outside interval");
            }
            out.push(v as i32);
        }
    } else {
        let (mean, valuebits, lengthbits) = if modern {
            (nibble(bits)?, 4, 4)
        } else {
            (bits.i(32)?, bits.u(3)?, bits.u(3)?)
        };
        if valuebits < 2 || lengthbits == 0 {
            return Err("invalid variable-width header");
        }
        let min = -(1 << (valuebits - 1));
        let max = (1 << (valuebits - 1)) - 1;
        let mut width = 0i32;
        while out.len() < n {
            loop {
                let delta = bits.i(valuebits)?;
                width = width.checked_add(delta).ok_or("field width overflow")?;
                if !(0..=32).contains(&width) {
                    return Err("invalid variable bit width");
                }
                if delta != min && delta != max {
                    break;
                }
            }
            let count = bits.u(lengthbits)? as usize;
            if count == 0 || count > n - out.len() {
                return Err("invalid variable-width run");
            }
            for _ in 0..count {
                out.push(bits.i(width as u32)?.wrapping_add(mean));
            }
        }
    }
    if bits.at != bits.limit {
        return Err("bitlength packet has unused bits");
    }
    Ok(out)
}
struct Entry {
    symbol: i32,
    value: i32,
    low: u32,
    high: u32,
}
fn context(bits: &mut Bits<'_>, cap: u64, modern: bool) -> Result<Vec<Entry>, &'static str> {
    let n = bits.u(16)?;
    if n as u64 > cap {
        return Err("arithmetic context budget exceeded");
    }
    let sb = if modern { 1 } else { bits.u(6)? };
    let cb = bits.u(6)?;
    let vb = bits.u(if modern { 7 } else { 6 })?;
    let min = bits.i(32)?;
    if n == 0 || sb > 32 || cb > 32 || vb > 32 {
        return Err("invalid arithmetic context header");
    }
    let mut out = Vec::new();
    let mut total = 0u32;
    let mut escapes = 0;
    for _ in 0..n {
        let symbol = if modern {
            if bits.u(1)? != 0 {
                -2
            } else {
                0
            }
        } else {
            (bits.u(sb)? as i32).wrapping_sub(2)
        };
        let count = bits.u(cb)?;
        if modern && symbol == -2 {
            escapes += 1;
            if escapes > 1 {
                return Err("duplicate arithmetic escape entries");
            }
        }
        let value = (bits.u(vb)? as i32).wrapping_add(min);
        let high = total
            .checked_add(count)
            .filter(|&v| v <= 16383)
            .ok_or("arithmetic frequency exceeds register precision")?;
        out.push(Entry {
            symbol,
            value,
            low: total,
            high,
        });
        total = high;
    }
    if total == 0 {
        return Err("zero arithmetic frequency total");
    }
    while bits.at % 8 != 0 {
        if bits.u(1)? != 0 {
            return Err("nonzero context alignment bits");
        }
    }
    Ok(out)
}
fn arithmetic(
    bits: &mut Bits<'_>,
    n: usize,
    context: &[Entry],
    oob: &[i32],
) -> Result<Vec<i32>, &'static str> {
    if bits.limit == 0 {
        return Err("empty arithmetic text");
    }
    let mut code = 0;
    for _ in 0..16 {
        code = (code << 1) | bits.arithmetic_bit()?;
    }
    let (mut low, mut high) = (0u32, 65535u32);
    let total = context.last().ok_or("empty arithmetic context")?.high;
    let mut out = Vec::new();
    out.try_reserve_exact(n)
        .map_err(|_| "integer allocation failed")?;
    let mut oob_at = 0;
    for _ in 0..n {
        if code < low || code > high {
            return Err("arithmetic code outside interval");
        }
        let range = high - low + 1;
        let scaled = (((code - low + 1) as u64 * total as u64 - 1) / range as u64) as u32;
        let at = context.partition_point(|entry| entry.high <= scaled);
        let e = context.get(at).ok_or("arithmetic symbol missing")?;
        high = low + (range as u64 * e.high as u64 / total as u64) as u32 - 1;
        low += (range as u64 * e.low as u64 / total as u64) as u32;
        loop {
            if (high ^ low) & 0x8000 == 0 {
            } else if low & 0x4000 != 0 && high & 0x4000 == 0 {
                code ^= 0x4000;
                low &= 0x3fff;
                high |= 0x4000;
            } else {
                break;
            }
            low = (low << 1) & 65535;
            high = ((high << 1) | 1) & 65535;
            code = ((code << 1) & 65535) | bits.arithmetic_bit()?;
        }
        out.push(if e.symbol == -2 {
            let v = *oob
                .get(oob_at)
                .ok_or("arithmetic escape without OOB value")?;
            oob_at += 1;
            v
        } else {
            e.value
        });
    }
    if oob_at != oob.len() {
        return Err("unused arithmetic OOB values");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> SourceSpan {
        SourceSpan {
            source_id: "fixture".into(),
            byte_domain: "mesh".into(),
            start_offset: 0,
            end_offset: 10000,
        }
    }
    fn cursor(bytes: &[u8]) -> Cursor<'_> {
        Cursor {
            bytes,
            at: 0,
            order: ByteOrder::Little,
            source: source(),
            modern: false,
            array_float_hash: false,
            externally_compressed: false,
        }
    }
    fn budget() -> Budget {
        Budget {
            remaining: 10000,
            packets: 1000,
            depth: 16,
            deepest: 0,
        }
    }
    fn raw(v: &[i32]) -> Vec<u8> {
        let mut out = (v.len() as i32).to_le_bytes().to_vec();
        if !v.is_empty() {
            out.push(0);
            out.extend((v.len() as i32 * 4).to_le_bytes());
            for v in v {
                out.extend(v.to_le_bytes());
            }
        }
        out
    }
    fn bitbytes(s: &str) -> Vec<u8> {
        let mut v = vec![0; s.len().div_ceil(8)];
        for (i, c) in s.bytes().enumerate() {
            if c == b'1' {
                v[i / 8] |= 1 << (7 - i % 8);
            }
        }
        v
    }
    #[test]
    fn modern_nibbler_mtf_and_arithmetic_packets_are_bounded() {
        let decode = |bytes: &[u8]| {
            let mut r = cursor(bytes);
            r.modern = true;
            let values = vector(&mut r, &mut budget(), Predictor::None)?;
            r.done()?;
            Ok::<_, ParseError>(values)
        };
        let fixed = coded(3, 1, "01110000100000010100");
        assert_eq!(decode(&fixed).unwrap(), [-2, 0, 2]);
        let variable = coded(2, 1, "101010001000101001");
        assert_eq!(decode(&variable).unwrap(), [3, 6]);
        let mut mtf = 5i32.to_le_bytes().to_vec();
        mtf.push(5);
        mtf.extend(raw(&[7, 9]));
        mtf.extend(raw(&[-1, -1, 1, 0, 1]));
        assert_eq!(decode(&mtf).unwrap(), [7, 9, 7, 7, 9]);
        let mut malformed = 1i32.to_le_bytes().to_vec();
        malformed.push(5);
        malformed.extend(raw(&[]));
        malformed.extend(raw(&[0]));
        assert!(decode(&malformed).is_err());
        assert!(decode(&coded(1, 1, &format!("0{}", "11111".repeat(8)))).is_err());
        let mut arith = coded(4, 3, "0101");
        arith.extend(bitbytes(&format!(
            "{:016b}{:06b}{:07b}{:032b}010011",
            2, 1, 1, 0
        )));
        assert_eq!(decode(&arith).unwrap(), [0, 1, 0, 1]); // No OOB packet without an escape.
        let mut escape = coded(2, 3, "");
        escape.extend(bitbytes(&format!(
            "{:016b}{:06b}{:07b}{:032b}11",
            1, 1, 0, 0
        )));
        let mut compressed = escape.clone();
        compressed.extend(2i32.to_le_bytes());
        compressed.extend(19i32.to_le_bytes());
        compressed.extend((-3i32).to_le_bytes());
        escape.extend(raw(&[19, -3]));
        assert_eq!(decode(&escape).unwrap(), [19, -3]);
        let mut r = cursor(&compressed);
        r.modern = true;
        r.externally_compressed = true;
        assert_eq!(
            vector(&mut r, &mut budget(), Predictor::None).unwrap(),
            [19, -3]
        );
        r.done().unwrap();
        for n in 0..mtf.len() {
            assert!(decode(&mtf[..n]).is_err());
        }
        let mut r = cursor(&mtf);
        r.modern = true;
        let mut b = budget();
        b.depth = 0;
        assert_eq!(
            vector(&mut r, &mut b, Predictor::None).unwrap_err().kind,
            K::LimitExceeded
        );
    }
    fn coded(n: i32, codec: u8, bits: &str) -> Vec<u8> {
        let mut out = n.to_le_bytes().to_vec();
        out.push(codec);
        out.extend((bits.len() as i32).to_le_bytes());
        let mut text = bitbytes(bits);
        text.resize(text.len().div_ceil(4) * 4, 0);
        for word in text.chunks_exact(4) {
            out.extend(word.iter().rev());
        }
        out
    }
    #[test]
    fn fixed_variable_arithmetic_and_zero_frequency() {
        let fixed = coded(
            3,
            1,
            &format!("0{:06b}{:06b}{}{}{}", 2, 3, "10", "010", "000010100"),
        ); // min=-2 max=2, deltas 0,2,4
        assert_eq!(
            vector(&mut cursor(&fixed), &mut budget(), Predictor::None).unwrap(),
            [-2, 0, 2]
        );
        let variable = coded(2, 1, &format!("1{:032b}0100110101000101001", 5));
        // Build explicitly: mean=5, delta widths +1,+1,0 -> 2, run=2, -2,+1.
        assert_eq!(
            vector(&mut cursor(&variable), &mut budget(), Predictor::None).unwrap(),
            [3, 6]
        );
        let mut arith = coded(4, 3, "0101");
        let ctx = format!(
            "{:016b}{:06b}{:06b}{:06b}{:032b}{}",
            3,
            2,
            1,
            1,
            0,
            "0000 1010 1111".replace(' ', "")
        );
        arith.extend(bitbytes(&ctx));
        arith.extend(raw(&[]));
        assert_eq!(
            vector(&mut cursor(&arith), &mut budget(), Predictor::None).unwrap(),
            [0, 1, 0, 1]
        );
    }
    #[test]
    fn wrapping_predictor_chopper_and_depth() {
        let bytes = raw(&[7, 8, 9, i32::MAX, 1, 1]);
        assert_eq!(
            vector(&mut cursor(&bytes), &mut budget(), Predictor::Lag1).unwrap(),
            [7, 8, 9, i32::MAX, i32::MIN, i32::MIN + 1]
        );
        let mut chop = 2i32.to_le_bytes().to_vec();
        chop.extend([4, 2]);
        chop.extend((-5i32).to_le_bytes());
        chop.push(4);
        chop.extend(raw(&[0, 1]));
        chop.extend(raw(&[1, 3]));
        assert_eq!(
            vector(&mut cursor(&chop), &mut budget(), Predictor::None).unwrap(),
            [-4, 2]
        );
        let mut nested = raw(&[1]);
        for _ in 0..5 {
            let mut bytes = 1i32.to_le_bytes().to_vec();
            bytes.extend([4, 0]);
            bytes.extend(nested);
            nested = bytes;
        }
        assert_eq!(
            vector(&mut cursor(&nested), &mut budget(), Predictor::None).unwrap(),
            [1]
        );
        let mut b = budget();
        b.depth = 3;
        assert_eq!(
            vector(&mut cursor(&nested), &mut b, Predictor::None)
                .unwrap_err()
                .kind,
            K::LimitExceeded
        );
        for end in 0..chop.len() {
            assert!(vector(&mut cursor(&chop[..end]), &mut budget(), Predictor::None).is_err());
        }
    }
    #[test]
    fn hostile_packet_lengths_widths_and_work() {
        for bytes in [vec![255, 255, 255, 255], raw(&[1, 2, 3])] {
            let mut b = budget();
            b.remaining = 0;
            assert!(vector(&mut cursor(&bytes), &mut b, Predictor::None).is_err());
        }
        let zero_run = coded(1, 1, &format!("1{:032b}01001100000", 0));
        assert!(vector(&mut cursor(&zero_run), &mut budget(), Predictor::None).is_err());
        let mut bad = raw(&[1]);
        bad[5..9].copy_from_slice(&8i32.to_le_bytes());
        assert!(vector(&mut cursor(&bad), &mut budget(), Predictor::None).is_err());
    }
}
