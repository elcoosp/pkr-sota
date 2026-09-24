//! Fuzz the blueprint loader. The fuzzer provides random bytes; we
//! attempt to mmap-load them as a blueprint and exercise the lookup
//! path. Any panic, abort, or unexpected error is a finding.

use libfuzzer_sys::fuzz_target;
use pkr_runtime::mmap::MmapError;

fuzz_target!(|data: &[u8]| {
    // Write to a temp file because MmapReader takes a path.
    let path = std::env::temp_dir().join(format!(
        "fuzz-blueprint-{}.bin",
        std::process::id(),
    ));
    if std::fs::write(&path, data).is_err() {
        return;
    }
    // Try to load — most random inputs will fail with InvalidMagic,
    // which is correct behavior. The goal is to catch inputs that
    // pass the magic check but then panic deeper in.
    match pkr_runtime::mmap::MmapReader::new(&path) {
        Ok(r) => {
            let h = pkr_runtime::SolverHandle::new(r);
            // Probe with a few hashes derived from the input.
            for chunk in data.chunks(8).take(8) {
                let mut b = [0u8; 8];
                b[..chunk.len()].copy_from_slice(chunk);
                let hash = u64::from_le_bytes(b);
                let _ = h.get_advice_fast(hash);
            }
        }
        Err(MmapError::InvalidMagic { .. }) => { /* expected */ }
        Err(MmapError::FileTooSmall) => { /* expected */ }
        Err(e) => {
            // Unexpected error type — surface for review.
            eprintln!("FUZZ: unexpected mmap error: {e:?}");
        }
    }
    let _ = std::fs::remove_file(&path);
});
