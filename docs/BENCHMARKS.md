# FolderFlow — Performance Benchmarks

> Baseline measurements recorded for `NFR-022` and `NFR-023` during Phase 8 (`P8-T04`).
> Per `NFR-022`, initial regression budgets are set from these first empirical measurements rather than promised in advance.

## Test environment

- **Machine / CPU:** Apple M4
- **Memory:** 16 GiB (`17,179,869,184` bytes)
- **OS:** macOS 27.0.1 (`26A434`) on APFS
- **Date:** 2026-11-14
- **Harness:** Criterion `0.8.2` (`src-tauri/benches/folderflow_benches.rs`) and Mach `/usr/bin/time -l` (`src-tauri/examples/bench_idle_monitor.rs`)

### Reproduction commands

```bash
cd src-tauri
cargo bench --bench folderflow_benches -- --warm-up-time 1 --measurement-time 2
cargo build --release --example bench_idle_monitor
/usr/bin/time -l ./target/release/examples/bench_idle_monitor 0
/usr/bin/time -l ./target/release/examples/bench_idle_monitor 10
/usr/bin/time -l ./target/release/deps/folderflow_benches-<hash> --bench --profile-time 1 --exact "bench_01_scan/100000"
/usr/bin/time -l ./target/release/deps/folderflow_benches-<hash> --bench --profile-time 1 --exact "bench_04_db_query/list_facts_for_folder/100000"
```

---

## BENCH-01 — Folder scan (`scan_folder`, APFS real files)

Measures full non-following `walkdir` + ignore evaluation + header classification into bounded 500-item batches across nested temporary directories.

| Dataset size | Mean time | 95% CI | Throughput | Regression budget |
|---:|---:|---:|---:|---:|
| 1,000 files | 24.81 ms | [24.36 ms, 25.29 ms] | 40.31k files/s | <= 50 ms |
| 10,000 files | 254.06 ms | [233.17 ms, 275.01 ms] | 39.36k files/s | <= 450 ms |
| 50,000 files | 1.328 s | [1.284 s, 1.372 s] | 37.66k files/s | <= 2.2 s |
| 100,000 files | 3.361 s | [2.859 s, 4.004 s] | 29.75k files/s | <= 6.0 s |

---

## BENCH-02 — Event pipeline latency (`Debouncer` + `StabilityTracker`)

Measures pushing `N` dirty-path events through `Debouncer::push` (capacity `10,000`), advancing quiet windows via `drain_due`, and tracking `Observation` stability cycles through `StabilityTracker`.

| Batch size | Mean batch time | Per-event amortized | Throughput | Regression budget |
|---:|---:|---:|---:|---:|
| 1,000 events | 1.400 ms | 1.40 us | 714.34k events/s | <= 2.5 ms |
| 10,000 events | 16.42 ms | 1.64 us | 609.10k events/s | <= 30 ms |
| 50,000 events | 86.10 ms | 1.72 us | 580.73k events/s | <= 150 ms |
| 100,000 events | 188.81 ms | 1.89 us | 529.64k events/s | <= 320 ms |

---

## BENCH-03 — Rule evaluation throughput (`evaluate_rules`, 10 active rules)

Evaluates 10 active rules combining extension, filename glob/regex, category, decimal file size, and relative date conditions across `N` normalized `FileFacts` records.

| Dataset size | Mean time | 95% CI | Throughput | Regression budget |
|---:|---:|---:|---:|---:|
| 1,000 files | 11.70 ms | [11.66 ms, 11.74 ms] | 85.49k files/s | <= 20 ms |
| 10,000 files | 103.52 ms | [102.74 ms, 104.35 ms] | 96.60k files/s | <= 180 ms |
| 50,000 files | 547.51 ms | [524.33 ms, 573.77 ms] | 91.32k files/s | <= 900 ms |
| 100,000 files | 1.290 s | [1.167 s, 1.438 s] | 77.55k files/s | <= 2.2 s |

---

## BENCH-04 — SQLite query latency (`repo_stats` & `repo_files`)

Measures `repo_stats::dashboard_stats`, `repo_stats::folder_summary`, and `repo_files::list_facts_for_folder` over indexed SQLite datasets of `1k`–`100k` `file_records` rows (`WAL` mode + `foreign_keys = ON`).

| Query | 1,000 rows | 10,000 rows | 50,000 rows | 100,000 rows | Budget @ 100k |
|---|---:|---:|---:|---:|---:|
| `dashboard_stats` | 1.630 ms | 4.777 ms | 21.62 ms | 51.39 ms | <= 90 ms |
| `folder_summary` | 700.7 us | 3.312 ms | 18.14 ms | 36.66 ms | <= 65 ms |
| `list_facts_for_folder` | 4.023 ms | 24.32 ms | 139.74 ms | 203.49 ms | <= 350 ms |

---

## BENCH-05 — Streaming BLAKE3 hashing (`full_hash_file` & `partial_hash_file`)

Uses a fixed 64 KiB read buffer for full hashes (`hasher::full_hash_file`) and 3 × 16 KiB sampled edge/middle windows for duplicate prefiltering (`hasher::partial_hash_file`).

| Operation | Payload size | Mean time | Logical throughput | Regression budget |
|---|---:|---:|---:|---:|
| `full_hash_file` | 16 MiB | 12.67 ms | 1.234 GiB/s | >= 900 MiB/s |
| `full_hash_file` | 64 MiB | 44.78 ms | 1.396 GiB/s | >= 900 MiB/s |
| `partial_hash_file` | 16 MiB | 170.1 us | 91.85 GiB/s | <= 500 us |
| `partial_hash_file` | 64 MiB | 149.2 us | 418.92 GiB/s | <= 500 us |

---

## BENCH-06 — Heavy-workload memory footprint (RSS)

Measured via Darwin `/usr/bin/time -l` during 100,000-file benchmarks.

| Workload | Max RSS | Peak memory footprint | Budget (Max RSS) |
|---|---:|---:|---:|
| `bench_01_scan/100000` (500-item bounded batch streaming) | 153,862,144 B (146.7 MiB) | 125,993,608 B (120.2 MiB) | <= 250 MiB |
| `bench_04_db_query/list_facts_for_folder/100000` (100k facts materialized) | 272,007,168 B (259.4 MiB) | 207,127,224 B (197.5 MiB) | <= 400 MiB |

---

## BENCH-07 — Idle monitoring CPU & resident memory

Measured via `bench_idle_monitor` running `MonitorService` + real macOS FSEvents watcher over a 100-file folder with zero filesystem events arriving:

| Run | Wall clock (`real`) | User CPU | System CPU | Instructions retired | Max RSS | Peak footprint |
|---|---:|---:|---:|---:|---:|---:|
| Startup baseline (`0 s` idle) | 0.42 s | 0.00 s | 0.02 s | 247,854,862 | 14.0 MiB | 4.5 MiB |
| Active FSEvents idle (`10 s` wait) | 10.04 s | 0.00 s | 0.01 s | 257,638,157 | 13.9 MiB | 4.3 MiB |

- **Net steady-state idle CPU over 10 s:** `+9.78M` retired instructions (~`< 0.1%` CPU, `0.00 s` user / `0.01 s` sys kernel accounting granularity).
- **Idle budget (`NFR-023`):** `<= 0.5%` average CPU and `<= 50 MiB` background RSS when idle.

