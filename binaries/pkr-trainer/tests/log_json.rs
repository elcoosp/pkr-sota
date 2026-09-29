//! Integration test for `--log-json`.
//!
//! Runs the trainer for a tiny iteration count and asserts every line
//! it writes that starts with `{` parses as JSON. Does not check event
//! semantics — that's done by eye in the sample output.

use std::process::Command;

fn workspace() -> std::path::PathBuf {
    let m = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    m.parent().unwrap().parent().unwrap().to_path_buf()
}

#[test]
fn log_json_emits_only_valid_json_objects() {
    let ws = workspace();
    let out_dir = tempfile::tempdir().expect("tempdir");
    let trainer = ws.join("target/release/pkr-trainer");
    if !trainer.exists() {
        // Skip if not built. This test is cheap but needs the binary.
        eprintln!("skipping: {} does not exist", trainer.display());
        return;
    }

    let smoke = ws.join("outputs/v0-smoke");
    if !smoke.join("centroids.bin").exists() {
        eprintln!("skipping: smoke abstraction not generated");
        return;
    }

    let out = Command::new(&trainer)
        // The smoke abstraction is k=8; the trainer refuses to run with
        // k < 100 unless this is set (see smoke.sh).
        .env("PKR_ALLOW_SMALL_K", "1")
        .env("PKR_ALLOW_EHS_FALLBACK", "1")
        .arg("--iterations").arg("50000")
        .arg("--seed").arg("42")
        .arg("--threads").arg("2")
        .arg("--capacity").arg("1000000")
        .arg("--eval-every").arg("50000")
        .arg("--eval-deals").arg("50")
        .arg("--centroids").arg(smoke.join("centroids.bin"))
        .arg("--preflop-table").arg(smoke.join("preflop_abstraction.bin"))
        .arg("--flop-table").arg(smoke.join("flop_abstraction.bin"))
        .arg("--turn-table").arg(smoke.join("turn_abstraction.bin"))
        .arg("--river-table").arg(smoke.join("river_buckets.bin"))
        .arg("--rank-table").arg(smoke.join("hand_ranks.bin"))
        .arg("--output").arg(out_dir.path().join("blueprint.bin"))
        .arg("--checkpoint").arg(out_dir.path().join("train.ckpt"))
        .arg("--fresh")
        .arg("--log-json")
        .output()
        .expect("spawn trainer");

    assert!(
        out.status.success(),
        "trainer exited non-zero: stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    let mut json_lines = 0usize;
    for line in stderr.lines() {
        if !line.starts_with('{') {
            continue;
        }
        json_lines += 1;
        let v: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("invalid JSON line: {e}\n{line}"));
        assert!(
            v.get("event").is_some(),
            "JSON line missing 'event' field:\n{line}"
        );
    }

    assert!(
        json_lines >= 2,
        "expected at least 2 JSON lines (progress + eval), got {json_lines}"
    );
}
