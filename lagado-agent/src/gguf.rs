//! gguf.rs — minimal GGUF metadata reader (the "model-reader").
//!
//! Parses ONLY the GGUF metadata KV header — never the tensor data — so the governor can
//! learn the model's REAL architecture (context window, layer count, experts, heads)
//! BEFORE launching llama-server and setting `-c`/`-ngl`. Zero dependencies
//! (sovereignty / supply-chain): the GGUF header is a simple, documented, little-endian
//! format. This exists to satisfy CLAUDE.md invariant #9 — DISCOVER, don't assume. The
//! model is swappable (H-1); the only way to stop hardcoding its context/layers/size is
//! to read them from the file.
//!
//! Format: magic `GGUF` | version u32 | tensor_count u64 | kv_count u64 | then kv_count
//! pairs of { key: (u64 len, bytes) ; value_type: u32 ; value }. All little-endian.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

/// Guard against a corrupt header asking us to allocate absurd buffers.
const MAX_STRING_BYTES: u64 = 64 * 1024 * 1024;
/// Guard against a corrupt array length looping forever.
const MAX_ARRAY_LEN: u64 = 64 * 1024 * 1024;

/// What the governor needs to plan without guessing. `Option` where a model may
/// legitimately omit a key; `expert_count` defaults to 0 (dense).
#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub arch: String,
    pub context_length: Option<u64>,
    pub block_count: Option<u64>,
    pub embedding_length: Option<u64>,
    pub head_count: Option<u64>,
    pub head_count_kv: Option<u64>,
    pub expert_count: u64,
    pub param_count: Option<u64>,
    pub file_bytes: u64,
}

impl ModelInfo {
    /// Mixture-of-Experts (≥2 experts) → `--cpu-moe` is a real lever.
    pub fn is_moe(&self) -> bool {
        self.expert_count >= 2
    }
}

/// Read GGUF metadata from a model file on disk.
pub fn read_metadata(path: &Path) -> Result<ModelInfo, String> {
    let file = File::open(path).map_err(|e| format!("gguf open {}: {e}", path.display()))?;
    let file_bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut r = BufReader::new(file);
    parse(&mut r, file_bytes).map_err(|e| format!("gguf parse {}: {e}", path.display()))
}

// ── parsing core (over any Read, so it's testable with an in-memory cursor) ──────

fn parse(r: &mut impl Read, file_bytes: u64) -> io::Result<ModelInfo> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if &magic != b"GGUF" {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a GGUF file"));
    }
    let _version = rd_u32(r)?;
    let _tensor_count = rd_u64(r)?;
    let kv_count = rd_u64(r)?;

    let mut meta: HashMap<String, Val> = HashMap::new();
    for _ in 0..kv_count {
        let key = rd_string(r)?;
        let vtype = rd_u32(r)?;
        let val = read_value(r, vtype)?;
        if !matches!(val, Val::Arr) {
            meta.insert(key, val); // arrays (tokenizer vocab etc.) are skipped, not stored
        }
    }

    let arch = match meta.get("general.architecture") {
        Some(Val::Str(s)) => s.clone(),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "missing general.architecture",
            ))
        }
    };

    let u = |k: &str| -> Option<u64> {
        match meta.get(k) {
            Some(Val::U(v)) => Some(*v),
            Some(Val::I(v)) if *v >= 0 => Some(*v as u64),
            _ => None,
        }
    };
    let ak = |suffix: &str| format!("{arch}.{suffix}");

    Ok(ModelInfo {
        context_length: u(&ak("context_length")),
        block_count: u(&ak("block_count")),
        embedding_length: u(&ak("embedding_length")),
        head_count: u(&ak("attention.head_count")),
        head_count_kv: u(&ak("attention.head_count_kv")),
        expert_count: u(&ak("expert_count")).unwrap_or(0),
        param_count: u("general.parameter_count"),
        file_bytes,
        arch,
    })
}

/// A parsed scalar/string value. Arrays are consumed but not retained (`Arr`).
enum Val {
    U(u64),
    I(i64),
    Str(String),
    Arr,
}

fn read_value(r: &mut impl Read, vtype: u32) -> io::Result<Val> {
    Ok(match vtype {
        0 => Val::U(rd_u8(r)? as u64),                 // UINT8
        1 => Val::I(rd_u8(r)? as i8 as i64),           // INT8
        2 => Val::U(rd_u16(r)? as u64),                // UINT16
        3 => Val::I(rd_u16(r)? as i16 as i64),         // INT16
        4 => Val::U(rd_u32(r)? as u64),                // UINT32
        5 => Val::I(rd_u32(r)? as i32 as i64),         // INT32
        6 => { rd_u32(r)?; Val::Arr }                  // FLOAT32 (unused → discard)
        7 => Val::U(rd_u8(r)? as u64),                 // BOOL
        8 => Val::Str(rd_string(r)?),                  // STRING
        9 => {                                         // ARRAY
            let elem = rd_u32(r)?;
            let len = rd_u64(r)?;
            if len > MAX_ARRAY_LEN {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "array too long"));
            }
            for _ in 0..len {
                read_value(r, elem)?; // consume + discard
            }
            Val::Arr
        }
        10 => Val::U(rd_u64(r)?),                       // UINT64
        11 => Val::I(rd_u64(r)? as i64),               // INT64
        12 => { rd_u64(r)?; Val::Arr }                 // FLOAT64 (unused → discard)
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown gguf value type {other}"),
            ))
        }
    })
}

fn rd_u8(r: &mut impl Read) -> io::Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}
fn rd_u16(r: &mut impl Read) -> io::Result<u16> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b)?;
    Ok(u16::from_le_bytes(b))
}
fn rd_u32(r: &mut impl Read) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}
fn rd_u64(r: &mut impl Read) -> io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}
fn rd_string(r: &mut impl Read) -> io::Result<String> {
    let n = rd_u64(r)?;
    if n > MAX_STRING_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "gguf string too long"));
    }
    let mut buf = vec![0u8; n as usize];
    r.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // ── builders for an in-memory GGUF (hermetic; no real file needed) ──
    fn push_str(b: &mut Vec<u8>, s: &str) {
        b.extend_from_slice(&(s.len() as u64).to_le_bytes());
        b.extend_from_slice(s.as_bytes());
    }
    fn kv_u32(b: &mut Vec<u8>, key: &str, v: u32) {
        push_str(b, key);
        b.extend_from_slice(&4u32.to_le_bytes()); // type UINT32
        b.extend_from_slice(&v.to_le_bytes());
    }
    fn kv_u64(b: &mut Vec<u8>, key: &str, v: u64) {
        push_str(b, key);
        b.extend_from_slice(&10u32.to_le_bytes()); // type UINT64
        b.extend_from_slice(&v.to_le_bytes());
    }
    fn kv_str(b: &mut Vec<u8>, key: &str, v: &str) {
        push_str(b, key);
        b.extend_from_slice(&8u32.to_le_bytes()); // type STRING
        push_str(b, v);
    }

    #[test]
    fn parses_real_shaped_header() {
        let mut b = Vec::new();
        b.extend_from_slice(b"GGUF");
        b.extend_from_slice(&3u32.to_le_bytes()); // version
        b.extend_from_slice(&0u64.to_le_bytes()); // tensor_count
        b.extend_from_slice(&6u64.to_le_bytes()); // kv_count
        kv_str(&mut b, "general.architecture", "lfm2");
        kv_u32(&mut b, "lfm2.context_length", 131072);
        kv_u32(&mut b, "lfm2.block_count", 24);
        kv_u32(&mut b, "lfm2.attention.head_count", 32);
        kv_u32(&mut b, "lfm2.expert_count", 8);
        kv_u64(&mut b, "general.parameter_count", 8_000_000_000);

        let info = parse(&mut Cursor::new(b), 4_700_000_000).unwrap();
        assert_eq!(info.arch, "lfm2");
        assert_eq!(info.context_length, Some(131072)); // read, not the 32768 assumption
        assert_eq!(info.block_count, Some(24));
        assert_eq!(info.head_count, Some(32));
        assert_eq!(info.expert_count, 8);
        assert!(info.is_moe());
        assert_eq!(info.param_count, Some(8_000_000_000));
        assert_eq!(info.file_bytes, 4_700_000_000);
    }

    #[test]
    fn skips_arrays_without_choking() {
        let mut b = Vec::new();
        b.extend_from_slice(b"GGUF");
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes());
        b.extend_from_slice(&2u64.to_le_bytes()); // kv_count = 2
        kv_str(&mut b, "general.architecture", "lfm2");
        // an array-of-string value (like a tokenizer vocab) must be consumed, not stored
        push_str(&mut b, "tokenizer.ggml.tokens");
        b.extend_from_slice(&9u32.to_le_bytes()); // ARRAY
        b.extend_from_slice(&8u32.to_le_bytes()); // elem type STRING
        b.extend_from_slice(&3u64.to_le_bytes()); // len 3
        push_str(&mut b, "a");
        push_str(&mut b, "bb");
        push_str(&mut b, "ccc");

        let info = parse(&mut Cursor::new(b), 0).unwrap();
        assert_eq!(info.arch, "lfm2");
        assert_eq!(info.context_length, None); // absent → None, NOT a guessed default
    }

    #[test]
    fn rejects_non_gguf() {
        let b = b"NOPE....".to_vec();
        assert!(parse(&mut Cursor::new(b), 0).is_err());
    }

    /// Reads the REAL on-disk GGUFs and prints what the governor should have been using
    /// all along. Run: `cargo test -p lagado-agent --lib gguf -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn reads_real_models() {
        let home = std::env::var("HOME").unwrap();
        let dir = format!("{home}/.laputa-secure/models");
        for name in [
            "LFM2-8B-A1B-Q4_K_M.gguf",
            "LFM2.5-1.2B-Instruct-Q4_K_M.gguf",
            "LFM2-ColBERT-350M-Q4_K_M.gguf",
            "LFM2-VL-450M-F16.gguf",
        ] {
            let p = std::path::PathBuf::from(&dir).join(name);
            if !p.exists() {
                eprintln!("(skip, absent) {name}");
                continue;
            }
            match read_metadata(&p) {
                Ok(m) => eprintln!(
                    "{name}\n  arch={} ctx={:?} layers={:?} n_embd={:?} heads={:?}/{:?} experts={} params={:?} bytes={}",
                    m.arch, m.context_length, m.block_count, m.embedding_length,
                    m.head_count, m.head_count_kv, m.expert_count, m.param_count, m.file_bytes,
                ),
                Err(e) => eprintln!("{name}: ERROR {e}"),
            }
        }
        // The headline assertion: the main model's REAL context is nowhere near 32768.
        let main = read_metadata(&std::path::PathBuf::from(&dir).join("LFM2-8B-A1B-Q4_K_M.gguf")).unwrap();
        assert!(main.context_length.unwrap() > 32768, "real ctx must exceed the old hardcoded 32768");
    }

    #[test]
    fn missing_architecture_is_an_error() {
        let mut b = Vec::new();
        b.extend_from_slice(b"GGUF");
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes()); // kv_count = 0
        assert!(parse(&mut Cursor::new(b), 0).is_err());
    }
}
