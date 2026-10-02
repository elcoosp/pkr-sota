//! Read a v4 blueprint back into a `CompactRegretTable`.
//!
//! Inverse of `writer::write_blueprint`. The blueprint stores an
//! average strategy per infoset as a quantized CDF; we invert it to
//! per-action probabilities and store them as strategy sums (the
//! table's average-strategy path normalizes sums, so storing
//! probabilities directly is equivalent for evaluation).
//!
//! This lets any saved artifact -- including a promoted blueprint
//! whose regret table was overwritten -- be re-evaluated at a
//! different deal count.

use crate::header::{
    FileHeader, FORMAT_VERSION_V3, FORMAT_VERSION_V4, HASH_ALGO_FNV1A64_INFOSET,
};
use pkr_cfr::table::CompactRegretTable;
use std::io;
use std::path::Path;

const MAGIC: &[u8; 8] = b"PKRSOTA1";

#[derive(Debug)]
pub enum ReadError {
    Io(io::Error),
    TooSmall,
    BadMagic([u8; 8]),
    UnsupportedVersion(u32),
    BadHashAlgo(u8),
    BadCounts { keys: u32, cdf: u32 },
    BadK(u8),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Io(e) => write!(f, "io: {e}"),
            ReadError::TooSmall => write!(f, "file too small"),
            ReadError::BadMagic(m) => write!(f, "bad magic {m:?}"),
            ReadError::UnsupportedVersion(v) => write!(f, "unsupported version {v}"),
            ReadError::BadHashAlgo(a) => write!(f, "bad hash_algo {a}"),
            ReadError::BadCounts { keys, cdf } => {
                write!(f, "key/cdf count mismatch: {keys} keys, {cdf} cdf bytes")
            }
            ReadError::BadK(k) => write!(f, "max_actions_k {k} != K"),
        }
    }
}
impl std::error::Error for ReadError {}
impl From<io::Error> for ReadError {
    fn from(e: io::Error) -> Self { ReadError::Io(e) }
}

/// Read a v4 blueprint into a fresh table. Returns the table and the
/// sorted key list.
pub fn read_blueprint(path: &Path) -> Result<(CompactRegretTable, Vec<u64>), ReadError> {
    let bytes = std::fs::read(path)?;
    let fh_size = std::mem::size_of::<FileHeader>();
    if bytes.len() < fh_size + 8 {
        return Err(ReadError::TooSmall);
    }

    let fh: FileHeader = *bytemuck::from_bytes(&bytes[..fh_size]);
    if &fh.magic != MAGIC {
        return Err(ReadError::BadMagic(fh.magic));
    }
    if fh.version < FORMAT_VERSION_V3 || fh.version > FORMAT_VERSION_V4 {
        return Err(ReadError::UnsupportedVersion(fh.version));
    }
    if fh.hash_algo != HASH_ALGO_FNV1A64_INFOSET {
        return Err(ReadError::BadHashAlgo(fh.hash_algo));
    }

    let anchors_size = 48usize;
    let fp_size = if fh.version >= FORMAT_VERSION_V4 { 40 } else { 0 };
    let after_header = fh_size + anchors_size + fp_size;

    // v4 carries a 40-byte AbstractionFingerprint. Read it and warn (do
    // not fail) if it disagrees with the current compile-time constants:
    // the blueprint is still loadable, but its infosets were hashed
    // under a different abstraction, so any evaluation would be
    // meaningless. Same footgun class that bit the v45 arena watcher and
    // the tournament table-dir default.
    if fh.version >= FORMAT_VERSION_V4 {
        let fp_off = fh_size + anchors_size;
        let stored: pkr_core::abstraction::AbstractionFingerprint =
            *bytemuck::from_bytes(&bytes[fp_off..fp_off + 40]);
        // Reader can't know current preflop_k; seed with stored so all
        // OTHER fingerprint axes are still validated. Passing
        // infoset_count made the warning fire on every valid load.
        let current =
            pkr_core::abstraction::AbstractionFingerprint::from_constants(stored.preflop_k);
        if stored != current {
            eprintln!(
                "WARNING: blueprint fingerprint differs from current build: {}",
                stored.describe_mismatch(&current)
            );
        }
    }

    let key_count = u32::from_le_bytes(
        bytes[after_header..after_header + 4].try_into().unwrap(),
    ) as usize;
    let cdf_size = u32::from_le_bytes(
        bytes[after_header + 4..after_header + 8].try_into().unwrap(),
    ) as usize;
    let k = fh.max_actions_k as usize;
    // The writer always emits K (=6). A file claiming more would
    // walk `off_sum` past the row into the next infoset's slots --
    // corruption from an untrusted blueprint. Reject it.
    if k != pkr_cfr::table::ACTION_K {
        return Err(ReadError::BadK(k as u8));
    }
    if cdf_size != key_count * k {
        return Err(ReadError::BadCounts {
            keys: key_count as u32,
            cdf: cdf_size as u32,
        });
    }

    let keys_start = after_header + 8;
    let cdf_start = keys_start + key_count * 8;
    if bytes.len() < cdf_start + cdf_size {
        return Err(ReadError::TooSmall);
    }

    let table = CompactRegretTable::with_capacity(key_count.max(1024));
    let mut keys = Vec::with_capacity(key_count);
    for i in 0..key_count {
        let off = keys_start + i * 8;
        let key = u64::from_le_bytes(bytes[off..off + 8].try_into().unwrap());
        keys.push(key);

        let cdf_off = cdf_start + i * k;
        let mut prev = 0u16;
        for a in 0..k {
            let c = bytes[cdf_off + a] as u16;
            let p = (c.saturating_sub(prev)) as f32 / 255.0;
            prev = c;
            if p > 0.0 {
                table.add_strategy_sum(key, a, p);
            }
        }
    }
    Ok((table, keys))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_core::abstraction::AbstractionFingerprint;

    #[test]
    fn roundtrip_preserves_strategies() {
        // Build a table with two known infosets, write, read, compare.
        let src = CompactRegretTable::with_capacity(1024);
        src.add_strategy_sum(111, 0, 0.5);
        src.add_strategy_sum(111, 1, 0.5);
        src.add_strategy_sum(222, 0, 1.0);
        src.add_strategy_sum(222, 2, 0.0);
        let keys = vec![111u64, 222];

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bp.bin");
        let fp = AbstractionFingerprint::from_constants(200);
        crate::writer::write_blueprint(path.to_str().unwrap(), &src, &keys, &fp).unwrap();

        let (dst, read_keys) = read_blueprint(&path).unwrap();
        assert_eq!(read_keys, keys);

        let mut s = [0.0f32; 6];
        dst.get_average_strategy_into(111, &mut s);
        assert!((s[0] - 0.5).abs() < 0.02, "s[0]={}", s[0]);
        assert!((s[1] - 0.5).abs() < 0.02, "s[1]={}", s[1]);

        dst.get_average_strategy_into(222, &mut s);
        assert!((s[0] - 1.0).abs() < 0.02, "s[0]={}", s[0]);
    }
}
