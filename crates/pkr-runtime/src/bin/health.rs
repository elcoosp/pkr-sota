//! `pkr-runtime-health <blueprint.bin> [queries]`
//!
//! Loads a blueprint via `MmapReader`, runs a synthetic lookup loop
//! against it, and prints the latency distribution. Exit code is 0
//! on a successful load + check, non-zero on failure.
//!
//! Intended for host-app self-checks: after a deploy, run this against
//! the blueprint the host will load and confirm p99 is in the expected
//! range. Also catches a truncated or corrupted mmap early (the
//! `MmapReader::new` call will fail with a specific `MmapError`).

use pkr_runtime::{MmapReader, SolverHandle};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: {} <blueprint.bin> [queries]", args[0]);
        eprintln!("       queries defaults to 100000");
        std::process::exit(2);
    }
    let path = &args[1];
    let queries: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(100_000);

    let reader = match MmapReader::new(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("load error: {e:?}");
            std::process::exit(1);
        }
    };

    let handle = SolverHandle::new(reader);
    let report = handle.health_check(queries);

    println!("{}", report.summary());

    // Loose sanity gates. A host that wants tighter bounds should call
    // the library directly.
    if report.key_count == 0 {
        eprintln!("warning: blueprint has 0 keys");
        std::process::exit(3);
    }
    if report.hits == 0 {
        eprintln!("warning: no query hit — key table format may be wrong");
        std::process::exit(4);
    }
}
