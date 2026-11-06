use crate::{
    container::ByteOrder,
    error::{ParseError, ParseErrorKind},
};

#[derive(Clone, Copy)]
pub(crate) struct Reader<'a> {
    pub data: &'a [u8],
    pub order: ByteOrder,
    pub source_id: &'a str,
    pub kind: ParseErrorKind,
}
impl Reader<'_> {
    pub fn slice(&self, offset: u64, length: u64) -> Result<&[u8], ParseError> {
        let end = offset.checked_add(length);
        let range = end
            .filter(|&end| end <= self.data.len() as u64)
            .and_then(|end| Some(usize::try_from(offset).ok()?..usize::try_from(end).ok()?));
        range.and_then(|range| self.data.get(range)).ok_or_else(|| {
            ParseError::at(
                self.kind,
                "unexpected_eof",
                "requested byte range lies outside the input",
                self.source_id,
                offset.min(self.data.len() as u64),
                0,
            )
        })
    }
    pub fn array<const N: usize>(&self, offset: u64) -> Result<[u8; N], ParseError> {
        let mut bytes = [0; N];
        bytes.copy_from_slice(self.slice(offset, N as u64)?);
        Ok(bytes)
    }
    pub fn u32(&self, offset: u64) -> Result<u32, ParseError> {
        let bytes = self.array(offset)?;
        Ok(match self.order {
            ByteOrder::Little => u32::from_le_bytes(bytes),
            ByteOrder::Big => u32::from_be_bytes(bytes),
        })
    }
    pub fn i32(&self, offset: u64) -> Result<i32, ParseError> {
        let bytes = self.array(offset)?;
        Ok(match self.order {
            ByteOrder::Little => i32::from_le_bytes(bytes),
            ByteOrder::Big => i32::from_be_bytes(bytes),
        })
    }
    pub fn u64(&self, offset: u64) -> Result<u64, ParseError> {
        let bytes = self.array(offset)?;
        Ok(match self.order {
            ByteOrder::Little => u64::from_le_bytes(bytes),
            ByteOrder::Big => u64::from_be_bytes(bytes),
        })
    }
}
