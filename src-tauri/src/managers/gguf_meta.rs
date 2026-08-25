//! Minimal, dependency-free GGUF metadata reader.
//!
//! Parses ONLY the GGUF key/value header section — the magic, the version, the
//! counts, and the `metadata_kv_count` key/value pairs that sit at the front of
//! the file, before any tensor info or tensor data. We never touch tensors, so
//! the exact same code works on a complete file on disk and on the leading
//! bytes of a remote file fetched with an HTTP Range request: a too-short
//! buffer surfaces cleanly as [`GgufError::Truncated`] instead of panicking.
//!
//! This is intentionally not a full GGUF library — it reads what Handy needs to
//! display a model's capabilities before download. Format reference: GGUF v2/v3,
//! little-endian. v1 (32-bit lengths) is not supported; every transcribe-cpp
//! model is v3.

use std::collections::HashMap;

/// GGUF magic: the ASCII bytes "GGUF" read as a little-endian u32.
const GGUF_MAGIC: u32 = 0x4655_4747;

// GGUF metadata value-type tags.
const T_UINT8: u32 = 0;
const T_INT8: u32 = 1;
const T_UINT16: u32 = 2;
const T_INT16: u32 = 3;
const T_UINT32: u32 = 4;
const T_INT32: u32 = 5;
const T_FLOAT32: u32 = 6;
const T_BOOL: u32 = 7;
const T_STRING: u32 = 8;
const T_ARRAY: u32 = 9;
const T_UINT64: u32 = 10;
const T_INT64: u32 = 11;
const T_FLOAT64: u32 = 12;

/// Sanity caps so malformed/garbage bytes can't be interpreted as an enormous
/// allocation or an unbounded "need more bytes" hint. Real ASR model metadata
/// is comfortably within these.
const MAX_STRING_LEN: usize = 64 * 1024 * 1024;
const MAX_ARRAY_LEN: u64 = 16 * 1024 * 1024;
const MAX_STORED_ARRAY_LEN: u64 = 4096;
const MAX_KV_COUNT: u64 = 1_000_000;

/// A parsed GGUF metadata value. Only the shapes Handy consumes are given
/// accessors; unrequested values are skipped without materializing them.
#[derive(Debug, Clone, PartialEq)]
pub enum GgufValue {
    U8(u8),
    I8(i8),
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    F32(f32),
    F64(f64),
    Bool(bool),
    String(String),
    Array(Vec<GgufValue>),
}

impl GgufValue {
    /// Interpret the value as a string, if it is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            GgufValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Interpret the value as a bool. GGUF stores bools as a single byte; some
    /// producers use a small integer type instead, so accept those too.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            GgufValue::Bool(b) => Some(*b),
            GgufValue::U8(v) => Some(*v != 0),
            GgufValue::I8(v) => Some(*v != 0),
            GgufValue::U32(v) => Some(*v != 0),
            GgufValue::I32(v) => Some(*v != 0),
            _ => None,
        }
    }

    /// Interpret the value as an array of strings (e.g. `general.languages`).
    /// Returns `None` if it isn't an array, or if any element isn't a string.
    pub fn as_string_array(&self) -> Option<Vec<String>> {
        match self {
            GgufValue::Array(items) => items
                .iter()
                .map(|v| v.as_str().map(str::to_string))
                .collect(),
            _ => None,
        }
    }
}

/// The parsed front-of-file metadata of a GGUF model. Only the key/value block
/// is retained — the version is validated during parsing and the tensor count is
/// skipped, since Handy reads capabilities purely from the KV pairs.
#[derive(Debug, Clone)]
pub struct GgufMetadata {
    /// Requested key/value metadata pairs, keyed by their GGUF key.
    pub kv: HashMap<String, GgufValue>,
}

impl GgufMetadata {
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.kv.get(key).and_then(GgufValue::as_str)
    }
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.kv.get(key).and_then(GgufValue::as_bool)
    }
    pub fn get_string_array(&self, key: &str) -> Option<Vec<String>> {
        self.kv.get(key).and_then(GgufValue::as_string_array)
    }
}

/// Why a parse did not produce metadata.
#[derive(Debug)]
pub enum GgufError {
    /// Not a GGUF file (bad magic).
    NotGguf,
    /// Unsupported GGUF version (we support v2 and v3).
    UnsupportedVersion(u32),
    /// The buffer ended before the metadata section was fully parsed. `needed`
    /// is a lower-bound hint for the total number of bytes to fetch and retry
    /// with — because element sizes vary, callers should also grow geometrically
    /// rather than trust this as the exact final size.
    Truncated { needed: usize },
    /// The bytes were malformed in a way that isn't simple truncation.
    Malformed(&'static str),
}

impl std::fmt::Display for GgufError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GgufError::NotGguf => write!(f, "not a GGUF file"),
            GgufError::UnsupportedVersion(v) => write!(f, "unsupported GGUF version {v}"),
            GgufError::Truncated { needed } => {
                write!(f, "buffer truncated, need at least {needed} bytes")
            }
            GgufError::Malformed(why) => write!(f, "malformed GGUF: {why}"),
        }
    }
}
impl std::error::Error for GgufError {}

/// A forward-only cursor that reports running off the end as `Truncated`
/// (carrying how many total bytes were wanted) rather than panicking.
struct ByteCursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> ByteCursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        ByteCursor { buf, pos: 0 }
    }

    fn truncated(&self, more: usize) -> GgufError {
        GgufError::Truncated {
            needed: self.pos.saturating_add(more),
        }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], GgufError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or(GgufError::Malformed("length overflow"))?;
        if end > self.buf.len() {
            return Err(self.truncated(n));
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u32(&mut self) -> Result<u32, GgufError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64, GgufError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn string_len(&mut self) -> Result<usize, GgufError> {
        let len = usize::try_from(self.u64()?)
            .map_err(|_| GgufError::Malformed("string length too large"))?;
        if len > MAX_STRING_LEN {
            return Err(GgufError::Malformed("string length too large"));
        }
        Ok(len)
    }

    /// A GGUF string: u64 length followed by that many UTF-8 bytes.
    fn string(&mut self) -> Result<String, GgufError> {
        let len = self.string_len()?;
        let bytes = self.take(len)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    fn skip_string(&mut self) -> Result<(), GgufError> {
        let len = self.string_len()?;
        self.take(len)?;
        Ok(())
    }
}

fn read_value(cur: &mut ByteCursor, value_type: u32) -> Result<GgufValue, GgufError> {
    Ok(match value_type {
        T_UINT8 => GgufValue::U8(cur.take(1)?[0]),
        T_INT8 => GgufValue::I8(cur.take(1)?[0] as i8),
        T_UINT16 => {
            let b = cur.take(2)?;
            GgufValue::U16(u16::from_le_bytes([b[0], b[1]]))
        }
        T_INT16 => {
            let b = cur.take(2)?;
            GgufValue::I16(i16::from_le_bytes([b[0], b[1]]))
        }
        T_UINT32 => GgufValue::U32(cur.u32()?),
        T_INT32 => GgufValue::I32(cur.u32()? as i32),
        T_FLOAT32 => GgufValue::F32(f32::from_bits(cur.u32()?)),
        T_BOOL => GgufValue::Bool(cur.take(1)?[0] != 0),
        T_STRING => GgufValue::String(cur.string()?),
        T_UINT64 => GgufValue::U64(cur.u64()?),
        T_INT64 => GgufValue::I64(cur.u64()? as i64),
        T_FLOAT64 => GgufValue::F64(f64::from_bits(cur.u64()?)),
        T_ARRAY => {
            let elem_type = cur.u32()?;
            if elem_type == T_ARRAY {
                return Err(GgufError::Malformed("nested arrays are not allowed"));
            }
            let len = cur.u64()?;
            if len > MAX_ARRAY_LEN {
                return Err(GgufError::Malformed("array length too large"));
            }
            if len > MAX_STORED_ARRAY_LEN {
                return Err(GgufError::Malformed("stored array length too large"));
            }
            // Don't pre-allocate the claimed length: a truncated buffer can
            // advertise a huge array it doesn't actually contain.
            let mut items = Vec::with_capacity(len.min(1024) as usize);
            for _ in 0..len {
                items.push(read_value(cur, elem_type)?);
            }
            GgufValue::Array(items)
        }
        _ => return Err(GgufError::Malformed("unknown value type")),
    })
}

fn scalar_size(value_type: u32) -> Option<usize> {
    match value_type {
        T_UINT8 | T_INT8 | T_BOOL => Some(1),
        T_UINT16 | T_INT16 => Some(2),
        T_UINT32 | T_INT32 | T_FLOAT32 => Some(4),
        T_UINT64 | T_INT64 | T_FLOAT64 => Some(8),
        _ => None,
    }
}

fn skip_value(cur: &mut ByteCursor, value_type: u32) -> Result<(), GgufError> {
    if let Some(size) = scalar_size(value_type) {
        cur.take(size)?;
        return Ok(());
    }

    match value_type {
        T_STRING => cur.skip_string(),
        T_ARRAY => {
            let elem_type = cur.u32()?;
            if elem_type == T_ARRAY {
                return Err(GgufError::Malformed("nested arrays are not allowed"));
            }
            let len = cur.u64()?;
            if len > MAX_ARRAY_LEN {
                return Err(GgufError::Malformed("array length too large"));
            }

            if let Some(size) = scalar_size(elem_type) {
                let bytes = usize::try_from(len)
                    .ok()
                    .and_then(|len| len.checked_mul(size))
                    .ok_or(GgufError::Malformed("length overflow"))?;
                cur.take(bytes)?;
            } else if elem_type == T_STRING {
                for _ in 0..len {
                    cur.skip_string()?;
                }
            } else {
                return Err(GgufError::Malformed("unknown array element type"));
            }
            Ok(())
        }
        _ => Err(GgufError::Malformed("unknown value type")),
    }
}

/// Parse the GGUF metadata header from `bytes`. `bytes` may be the whole file or
/// just a leading prefix (e.g. an HTTP Range fetch); a prefix too short to hold
/// the full metadata section returns [`GgufError::Truncated`].
///
/// Only keys listed in `wanted_keys` are materialized. Other values are skipped
/// with checked cursor movement so large tokenizer metadata arrays do not become
/// Handy allocations while probing a few capability fields.
pub fn parse_header(bytes: &[u8], wanted_keys: &[&str]) -> Result<GgufMetadata, GgufError> {
    let mut cur = ByteCursor::new(bytes);

    let magic = cur.u32()?;
    if magic != GGUF_MAGIC {
        return Err(GgufError::NotGguf);
    }
    let version = cur.u32()?;
    if version != 2 && version != 3 {
        return Err(GgufError::UnsupportedVersion(version));
    }
    // Tensor count precedes the KV count in the header; read past it (we never
    // touch tensors) to reach the metadata block.
    cur.u64()?;
    let kv_count = cur.u64()?;
    if kv_count > MAX_KV_COUNT {
        return Err(GgufError::Malformed("absurd metadata kv count"));
    }

    let mut kv = HashMap::with_capacity(wanted_keys.len());
    for _ in 0..kv_count {
        let key = cur.string()?;
        let value_type = cur.u32()?;
        if wanted_keys.contains(&key.as_str()) {
            let value = read_value(&mut cur, value_type)?;
            kv.insert(key, value);
            if kv.len() == wanted_keys.len() {
                break;
            }
        } else {
            skip_value(&mut cur, value_type)?;
        }
    }

    Ok(GgufMetadata { kv })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_str(out: &mut Vec<u8>, s: &str) {
        out.extend_from_slice(&(s.len() as u64).to_le_bytes());
        out.extend_from_slice(s.as_bytes());
    }

    fn write_value(out: &mut Vec<u8>, v: &GgufValue) {
        match v {
            GgufValue::Bool(b) => {
                out.extend_from_slice(&T_BOOL.to_le_bytes());
                out.push(*b as u8);
            }
            GgufValue::String(s) => {
                out.extend_from_slice(&T_STRING.to_le_bytes());
                push_str(out, s);
            }
            GgufValue::U32(n) => {
                out.extend_from_slice(&T_UINT32.to_le_bytes());
                out.extend_from_slice(&n.to_le_bytes());
            }
            GgufValue::Array(items) => {
                // Tests only build string arrays.
                out.extend_from_slice(&T_ARRAY.to_le_bytes());
                out.extend_from_slice(&T_STRING.to_le_bytes());
                out.extend_from_slice(&(items.len() as u64).to_le_bytes());
                for it in items {
                    if let GgufValue::String(s) = it {
                        push_str(out, s);
                    }
                }
            }
            _ => unimplemented!("test builder only supports a few types"),
        }
    }

    fn build_gguf(kvs: &[(&str, GgufValue)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&GGUF_MAGIC.to_le_bytes());
        out.extend_from_slice(&3u32.to_le_bytes()); // version
        out.extend_from_slice(&0u64.to_le_bytes()); // tensor_count
        out.extend_from_slice(&(kvs.len() as u64).to_le_bytes());
        for (k, v) in kvs {
            push_str(&mut out, k);
            write_value(&mut out, v);
        }
        out
    }

    #[test]
    fn parses_basic_kv() {
        let data = build_gguf(&[
            ("general.architecture", GgufValue::String("whisper".into())),
            ("stt.capability.translate", GgufValue::Bool(true)),
            (
                "general.languages",
                GgufValue::Array(vec![
                    GgufValue::String("en".into()),
                    GgufValue::String("de".into()),
                ]),
            ),
        ]);
        let meta = parse_header(
            &data,
            &[
                "general.architecture",
                "stt.capability.translate",
                "general.languages",
            ],
        )
        .unwrap();
        assert_eq!(meta.get_str("general.architecture"), Some("whisper"));
        assert_eq!(meta.get_bool("stt.capability.translate"), Some(true));
        assert_eq!(
            meta.get_string_array("general.languages"),
            Some(vec!["en".to_string(), "de".to_string()])
        );
        assert_eq!(meta.get_str("missing.key"), None);
    }

    #[test]
    fn rejects_non_gguf() {
        assert!(matches!(
            parse_header(b"not a gguf file at all", &["general.architecture"]),
            Err(GgufError::NotGguf)
        ));
    }

    #[test]
    fn reports_truncation_with_hint() {
        let data = build_gguf(&[("general.architecture", GgufValue::String("whisper".into()))]);
        let err = parse_header(&data[..data.len() - 3], &["general.architecture"]).unwrap_err();
        match err {
            GgufError::Truncated { needed } => assert!(needed >= data.len() - 3),
            other => panic!("expected Truncated, got {other:?}"),
        }
    }

    #[test]
    fn empty_buffer_is_truncated_not_panic() {
        assert!(matches!(
            parse_header(&[], &["general.architecture"]),
            Err(GgufError::Truncated { .. })
        ));
    }

    #[test]
    fn stops_after_wanted_keys_without_reading_later_large_arrays() {
        let mut data = build_gguf(&[("general.architecture", GgufValue::String("whisper".into()))]);
        // Patch kv_count from 1 to 2 and append an enormous, intentionally
        // truncated array. The parser should stop after the wanted first key.
        data[16..24].copy_from_slice(&2u64.to_le_bytes());
        push_str(&mut data, "tokenizer.ggml.tokens");
        data.extend_from_slice(&T_ARRAY.to_le_bytes());
        data.extend_from_slice(&T_STRING.to_le_bytes());
        data.extend_from_slice(&MAX_ARRAY_LEN.to_le_bytes());

        let meta = parse_header(&data, &["general.architecture"]).unwrap();
        assert_eq!(meta.get_str("general.architecture"), Some("whisper"));
    }

    #[test]
    fn skips_unwanted_arrays_without_storing_them() {
        let mut data = Vec::new();
        data.extend_from_slice(&GGUF_MAGIC.to_le_bytes());
        data.extend_from_slice(&3u32.to_le_bytes()); // version
        data.extend_from_slice(&0u64.to_le_bytes()); // tensor_count
        data.extend_from_slice(&2u64.to_le_bytes()); // kv_count

        push_str(&mut data, "unwanted.bytes");
        data.extend_from_slice(&T_ARRAY.to_le_bytes());
        data.extend_from_slice(&T_UINT8.to_le_bytes());
        data.extend_from_slice(&(MAX_STORED_ARRAY_LEN + 1).to_le_bytes());
        data.extend(std::iter::repeat(0).take((MAX_STORED_ARRAY_LEN + 1) as usize));

        push_str(&mut data, "general.architecture");
        data.extend_from_slice(&T_STRING.to_le_bytes());
        push_str(&mut data, "whisper");

        let meta = parse_header(&data, &["general.architecture"]).unwrap();
        assert_eq!(meta.get_str("general.architecture"), Some("whisper"));
        assert_eq!(meta.kv.len(), 1);
    }
}
