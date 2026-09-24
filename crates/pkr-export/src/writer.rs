use crate::header::{
    AnchorsSection, FileHeader, ANCHORS, FORMAT_VERSION_V4, HASH_ALGO_FNV1A64_INFOSET,
};
use pkr_cfr::table::CompactRegretTable;
use pkr_core::abstraction::AbstractionFingerprint;
use std::fs::File;
use std::io::Write;

const MAGIC: &[u8; 8] = b"PKRSOTA1";
const K: usize = 6;

/// Quantise a probability vector to a monotone u8 CDF that ALWAYS
/// closes at 255 on the last action with non-zero probability. This
/// prevents rounding slack from landing on an action whose probability
/// is zero (e.g. an illegal bucket).
pub(crate) fn quantize_cdf(strat: &[f32; K]) -> [u8; K] {
    let mut out = [0u8; K];
    let total: f32 = strat.iter().sum();
    if total.is_nan() || total <= 0.0 {
        for a in 0..K {
            out[a] = ((((a + 1) as f32) / K as f32) * 255.0).round() as u8;
        }
        out[K - 1] = 255;
        return out;
    }
    let mut cum = 0.0f32;
    let mut prev = 0u8;
    for a in 0..K {
        cum += strat[a] / total;
        let b = (cum * 255.0).round().clamp(0.0, 255.0) as u8;
        out[a] = b.max(prev);
        prev = out[a];
    }
    // Close on the last action that actually has probability.
    let last = (0..K).rev().find(|&a| strat[a] > 0.0).unwrap_or(K - 1);
    for a in last..K {
        out[a] = 255;
    }
    out
}

/// Write a complete blueprint file.
///
/// v4 layout:
///   [FileHeader:32][AnchorsSection:48][Fingerprint:40][key_count:u32][cdf_size:u32][keys][cdf]
///
/// Keys are u64 ascending; CDFs are K bytes per key with monotonic
/// non-decreasing values ending at 255.
pub fn write_blueprint(
    path: &str,
    table: &CompactRegretTable,
    keys: &[u64],
    fingerprint: &AbstractionFingerprint,
) -> std::io::Result<()> {
    // Sort defensively (reader uses binary search).
    let mut sorted_keys: Vec<u64> = keys.to_vec();
    sorted_keys.sort_unstable();
    let keys: &[u64] = &sorted_keys;
    let num_keys = keys.len();

    let mut cdf_bytes: Vec<u8> = Vec::with_capacity(num_keys * K);
    let mut key_bytes: Vec<u8> = Vec::with_capacity(num_keys * 8);

    let mut strat = [0.0f32; K];
    for &key in keys {
        key_bytes.extend_from_slice(&key.to_le_bytes());
        table.get_average_strategy_into(key, &mut strat);
        cdf_bytes.extend_from_slice(&quantize_cdf(&strat));
    }

    let file_header = FileHeader {
        magic: *MAGIC,
        version: FORMAT_VERSION_V4,
        variant_id: 0,
        infoset_count: num_keys as u64,
        max_actions_k: K as u8,
        hash_algo: HASH_ALGO_FNV1A64_INFOSET,
        _padding: [0u8; 6],
    };

    let anchors = AnchorsSection { anchors: ANCHORS };

    let tmp = format!("{path}.tmp");
    {
        let mut file = File::create(&tmp)?;
        file.write_all(bytemuck::bytes_of(&file_header))?;
        file.write_all(bytemuck::bytes_of(&anchors))?;
        file.write_all(bytemuck::bytes_of(fingerprint))?;
        file.write_all(&(num_keys as u32).to_le_bytes())?;
        file.write_all(&((K * num_keys) as u32).to_le_bytes())?;
        file.write_all(&key_bytes)?;
        file.write_all(&cdf_bytes)?;
        file.flush()?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_section_size_is_48() {
        assert_eq!(std::mem::size_of::<AnchorsSection>(), 48);
    }

    #[test]
    fn file_header_size_is_32() {
        assert_eq!(std::mem::size_of::<FileHeader>(), 32);
    }
}

#[cfg(test)]
mod v4_layout_tests {
    use super::*;
    use pkr_cfr::table::CompactRegretTable;

    /// The v4 blueprint file size is a deterministic function of the
    /// key count:
    ///
    ///   [FileHeader:32][Anchors:48][Fingerprint:40][kc:4][cs:4][keys:8N][cdf:6N]
    ///   = 128 + 14N
    ///
    /// This pins the on-disk layout. If any section size changes, this
    /// test fails and the format version must be bumped.
    #[test]
    fn v4_size_accounting_is_exact() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let table = CompactRegretTable::with_capacity(64);
        // Touch a few keys so get_average_strategy_into returns real CDFs.
        for k in [1u64, 42, 999] {
            table.add_strategy_sum(k, 0, 0.5);
            table.add_strategy_sum(k, 1, 0.5);
        }
        let keys: Vec<u64> = vec![1, 42, 999];
        let fp = pkr_core::abstraction::AbstractionFingerprint::from_constants(4);

        let _ = write_blueprint(tmp.path().to_str().unwrap(), &table, &keys, &fp);

        let expected = 32 + 48 + 40 + 4 + 4 + 8 * keys.len() + 6 * keys.len();
        let actual = std::fs::metadata(tmp.path()).unwrap().len() as usize;
        assert_eq!(
            actual,
            expected,
            "v4 blueprint size mismatch: got {}, expected {} (N={})",
            actual,
            expected,
            keys.len()
        );
    }

    /// Section offsets are stable: reading back the fingerprint must
    /// yield the one written.
    #[test]
    fn v4_fingerprint_roundtrips() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let table = CompactRegretTable::with_capacity(16);
        let keys: Vec<u64> = vec![100];
        let fp = pkr_core::abstraction::AbstractionFingerprint::from_constants(7);
        let _ = write_blueprint(tmp.path().to_str().unwrap(), &table, &keys, &fp);

        let bytes = std::fs::read(tmp.path()).unwrap();
        // Fingerprint at offset 32 + 48 = 80, length 40.
        let fp_bytes = &bytes[80..120];
        let stored: &pkr_core::abstraction::AbstractionFingerprint = bytemuck::from_bytes(fp_bytes);
        assert_eq!(*stored, fp);
    }
}

#[cfg(test)]
mod cdf_tests {
    use super::*;

    #[test]
    fn cdf_is_monotone_and_closes_on_last_nonzero_action() {
        let s = [0.2f32, 0.3, 0.5, 0.0, 0.0, 0.0];
        let c = quantize_cdf(&s);
        assert!(c.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(c[2], 255);
        assert_eq!(c[5], 255);
        assert!(c[1] < 255);
    }

    #[test]
    fn all_zero_is_uniform_and_closed() {
        let c = quantize_cdf(&[0.0; K]);
        assert_eq!(c[K - 1], 255);
    }

    #[test]
    fn rounds_to_nearest_without_overshoot() {
        let s = [0.333f32, 0.333, 0.334, 0.0, 0.0, 0.0];
        let c = quantize_cdf(&s);
        for i in 0..K {
            assert!(c[i] >= c[i.saturating_sub(1)]);
        }
        assert_eq!(c[2], 255);
    }
}
