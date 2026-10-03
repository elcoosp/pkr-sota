//! CI gate: the production CFR path must converge on Leduc.
//! Catches silent regressions in dcfr.rs / table.rs / traversal.rs.
//! 200k iters ~4 s; base config reads ~47 mchips, threshold 70.
use std::process::Command;

#[test]
fn leduc_converges() {
    let exe = env!("CARGO_BIN_EXE_pkr-leduc-check");
    let out = Command::new(exe)
        .args(["200000", "256", "1", "200000"])
        .output()
        .expect("run leduc");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let last = stdout.lines().last().unwrap_or("");
    eprintln!("{last}");
    let m: f64 = last
        .split("expl_mchips=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .expect("parse expl_mchips");
    assert!(m < 70.0, "Leduc exploitability regressed: {m} mchips (limit 70)");
    assert!(m > 0.0, "exploitability is zero - solver did nothing");
}
