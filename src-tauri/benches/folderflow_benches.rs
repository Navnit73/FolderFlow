//! FolderFlow Criterion benchmarks (`P8-T04`, `NFR-022`: `BENCH-01..05`).
//!
//! Scales: 1k, 10k, 50k, 100k files covering scanner throughput, debounce +
//! stability pipeline latency, rule evaluator throughput, SQLite query latency,
//! and streaming BLAKE3 hashing speed.

use std::fs::{self, OpenOptions};
use std::hint::black_box;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};
use tempfile::TempDir;
use tokio::runtime::Runtime;

use folderflow_lib::categories::builtin_categories;
use folderflow_lib::database::{open_in_memory, repo_files, repo_folders, repo_stats};
use folderflow_lib::duplicates::hasher::{full_hash_file, partial_hash_file};
use folderflow_lib::engine::classifier::{Classifier, Confidence};
use folderflow_lib::engine::debounce::Debouncer;
use folderflow_lib::engine::fs_view::{EntryKind, FileFacts as FsFileFacts, RealFs};
use folderflow_lib::engine::ignore::IgnoreRules;
use folderflow_lib::engine::scanner::{scan, ScanOptions, ScannedFile};
use folderflow_lib::engine::stability::{Observation, StabilityTracker};
use folderflow_lib::rules::evaluator::evaluate_rules;
use folderflow_lib::rules::models::{
    Action, Condition, ConditionNode, DateConditionOp, DateField, DateSource, ExecutionMode,
    FileFacts as RuleFileFacts, LocationOp, Rule, RuleOrigin, SizeOp, SizeUnit, TextFieldKind,
    TextOp,
};
use folderflow_lib::utils::clock::FixedClock;

const SCALES: &[usize] = &[1_000, 10_000, 50_000, 100_000];
const FILES_PER_SUBDIR: usize = 1_000;
const BENCH_NOW_MS: i64 = 1_735_689_600_000; // 2025-01-01T00:00:00Z

fn write_fixture_file(path: &Path, bytes: &[u8]) {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("create benchmark fixture file");
    f.write_all(bytes).expect("write benchmark fixture bytes");
}

fn create_scan_tree(count: usize) -> TempDir {
    let dir = tempfile::tempdir().expect("create temp scan root");
    let subdirs = count.div_ceil(FILES_PER_SUBDIR);
    let extensions = ["pdf", "jpg", "png", "txt", "zip", "docx", "csv", "mp3"];
    let mut created = 0usize;

    for s in 0..subdirs {
        let sub = dir.path().join(format!("bucket_{s:03}"));
        fs::create_dir_all(&sub).expect("create benchmark subdir");
        let target = FILES_PER_SUBDIR.min(count.saturating_sub(created));
        for i in 0..target {
            let ext = extensions[(created + i) % extensions.len()];
            let p = sub.join(format!("item_{created:06}_{i}.{ext}"));
            // Tiny valid header signature for pdf/png or plain text so classifier exercises both header & extension paths.
            let payload: &[u8] = match ext {
                "pdf" => b"%PDF-1.7\n%bench\n",
                "png" => b"\x89PNG\r\n\x1a\nbench",
                _ => b"folderflow benchmark payload\n",
            };
            write_fixture_file(&p, payload);
            created += 1;
        }
    }
    dir
}

fn sample_rule_facts(count: usize, root: &str) -> Vec<RuleFileFacts> {
    let extensions = [
        ("pdf", "documents", "application/pdf", "Invoice_2025"),
        ("png", "images", "image/png", "Screenshot 2025-01-01"),
        ("zip", "archives", "application/zip", "release-bundle"),
        ("csv", "spreadsheets", "text/csv", "q4-metrics"),
        ("mp4", "videos", "video/mp4", "screen-recording"),
    ];
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let (ext, cat, mime, prefix) = extensions[i % extensions.len()];
        let name = format!("{prefix}_{i:06}.{ext}");
        let stem = format!("{prefix}_{i:06}");
        out.push(RuleFileFacts {
            path: format!("{root}/sub_{:02}/{name}", i % 50),
            name,
            stem,
            extension: ext.to_owned(),
            mime: Some(mime.to_owned()),
            category: cat.to_owned(),
            size: 120_000 + ((i as u64 % 5_000) * 1024),
            created_ms: Some(BENCH_NOW_MS - 3 * 86_400_000),
            modified_ms: BENCH_NOW_MS - ((i as i64 % 45) * 86_400_000),
            added_ms: Some(BENCH_NOW_MS - 86_400_000),
            is_hidden: false,
            is_duplicate: i % 17 == 0,
            content_hash: None,
            source_folder_id: "bench-folder".to_owned(),
            source_folder_path: root.to_owned(),
            source_folder_name: "Downloads".to_owned(),
        });
    }
    out
}

fn bench_ruleset(dest_root: &str) -> Vec<Rule> {
    let mut rules = Vec::with_capacity(10);
    let defs: &[(&str, ConditionNode, &str)] = &[
        (
            "Screenshots",
            ConditionNode::All {
                children: vec![
                    ConditionNode::Leaf {
                        condition: Condition::Category {
                            category_id: "images".into(),
                            negate: false,
                        },
                    },
                    ConditionNode::Leaf {
                        condition: Condition::Text {
                            field: TextFieldKind::Name,
                            op: TextOp::StartsWith,
                            value: "Screenshot".into(),
                            case_sensitive: false,
                        },
                    },
                ],
            },
            "{year}/{month}",
        ),
        (
            "Invoices",
            ConditionNode::All {
                children: vec![
                    ConditionNode::Leaf {
                        condition: Condition::ExtensionGroup {
                            extensions: vec!["pdf".into(), "docx".into()],
                        },
                    },
                    ConditionNode::Leaf {
                        condition: Condition::Text {
                            field: TextFieldKind::Name,
                            op: TextOp::MatchesRegex,
                            value: r"^Invoice_\d+".into(),
                            case_sensitive: true,
                        },
                    },
                ],
            },
            "Finance/{year}",
        ),
        (
            "Large Videos",
            ConditionNode::All {
                children: vec![
                    ConditionNode::Leaf {
                        condition: Condition::Category {
                            category_id: "videos".into(),
                            negate: false,
                        },
                    },
                    ConditionNode::Leaf {
                        condition: Condition::Size {
                            op: SizeOp::GreaterThan,
                            value: 1,
                            max_value: None,
                            unit: SizeUnit::Megabytes,
                        },
                    },
                ],
            },
            "Media/Videos",
        ),
        (
            "Archives",
            ConditionNode::Leaf {
                condition: Condition::ExtensionGroup {
                    extensions: vec!["zip".into(), "tar".into(), "gz".into()],
                },
            },
            "Archives",
        ),
        (
            "Spreadsheets",
            ConditionNode::All {
                children: vec![
                    ConditionNode::Leaf {
                        condition: Condition::FilenameGlob {
                            pattern: "*metrics*".into(),
                            case_sensitive: false,
                        },
                    },
                    ConditionNode::Leaf {
                        condition: Condition::Date {
                            field: DateField::Modified,
                            op: DateConditionOp::NewerThanDays { days: 60 },
                        },
                    },
                ],
            },
            "Reports/{year}-{month}",
        ),
    ];

    for idx in 0..10 {
        let (label, cond, subpath) = &defs[idx % defs.len()];
        // First 5 rules match actively; second 5 inspect subdir/location/not rules.
        let conditions = if idx < 5 {
            cond.clone()
        } else {
            ConditionNode::All {
                children: vec![
                    ConditionNode::Leaf {
                        condition: Condition::Location {
                            op: LocationOp::InsideSubdirectory { subpath: None },
                        },
                    },
                    ConditionNode::Not {
                        child: Box::new(ConditionNode::Leaf {
                            condition: Condition::EmptyFile { is_empty: true },
                        }),
                    },
                    cond.clone(),
                ],
            }
        };
        rules.push(Rule {
            id: format!("rule-{idx:02}"),
            name: format!("{label} #{idx}"),
            description: None,
            enabled: true,
            priority: idx as i32,
            source_folders: Vec::new(),
            conditions,
            actions: vec![Action::MoveIntoDynamicFolder {
                root: Some(format!("{dest_root}/Sorted")),
                subpath_template: (*subpath).to_owned(),
            }],
            date_source: DateSource::Modified,
            execution_mode: ExecutionMode::FirstMatch,
            origin: RuleOrigin::User,
            last_run_at: None,
            stat_matched: 0,
            stat_succeeded: 0,
            stat_failed: 0,
            created_at: BENCH_NOW_MS,
            updated_at: BENCH_NOW_MS,
        });
    }
    rules
}

fn sample_scanned_files(count: usize, root: &str) -> Vec<ScannedFile> {
    let cats = ["documents", "images", "archives", "spreadsheets", "videos"];
    let exts = ["pdf", "png", "zip", "csv", "mp4"];
    (0..count)
        .map(|i| {
            let ext = exts[i % exts.len()];
            let name = format!("tracked_{i:06}.{ext}");
            ScannedFile {
                facts: FsFileFacts {
                    path: PathBuf::from(format!("{root}/sub_{:02}/{name}", i % 50)),
                    name,
                    extension: Some(ext.to_owned()),
                    kind: EntryKind::File,
                    size: 4096 + (i as u64 * 17),
                    dev: 1,
                    ino: 100_000 + i as u64,
                    created_ns: Some(1_735_000_000_000_000_000),
                    modified_ns: 1_735_500_000_000_000_000 + i as i64,
                    is_hidden: false,
                },
                category: cats[i % cats.len()].to_owned(),
                mime: None,
                confidence: Confidence::High,
            }
        })
        .collect()
}

/// BENCH-01: Recursive folder scan & classification across 1k..100k files on disk.
fn bench_scan(c: &mut Criterion) {
    let classifier = Classifier::new(builtin_categories());
    let opts = ScanOptions {
        recursive: true,
        rules: IgnoreRules::default(),
    };
    let mut group = c.benchmark_group("bench_01_scan");
    group.sampling_mode(SamplingMode::Flat);

    for &n in SCALES {
        let tree = create_scan_tree(n);
        group.sample_size(if n >= 50_000 { 10 } else { 20 });
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let summary = scan(tree.path(), &opts, &classifier, |batch| {
                    black_box(batch.len());
                    Ok(())
                })
                .expect("benchmark scan succeeds");
                black_box(summary.files);
            });
        });
    }
    group.finish();
}

/// BENCH-02: Debouncer + StabilityTracker pipeline throughput & latency across 1k..100k events.
fn bench_event_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("bench_02_event_pipeline");
    group.sampling_mode(SamplingMode::Flat);

    for &n in SCALES {
        let paths: Vec<PathBuf> = (0..n)
            .map(|i| PathBuf::from(format!("/tmp/ff-bench/file_{i:06}.pdf")))
            .collect();
        let facts_vec: Vec<FsFileFacts> = paths
            .iter()
            .enumerate()
            .map(|(i, p)| FsFileFacts {
                path: p.clone(),
                name: format!("file_{i:06}.pdf"),
                extension: Some("pdf".into()),
                kind: EntryKind::File,
                size: 8192,
                dev: 1,
                ino: i as u64 + 1,
                created_ns: Some(1_700_000_000_000_000_000),
                modified_ns: 1_700_000_000_000_000_000,
                is_hidden: false,
            })
            .collect();

        group.sample_size(if n >= 50_000 { 10 } else { 20 });
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let t0 = Instant::now();
                let mut debouncer = Debouncer::new(Duration::ZERO, n + 1);
                let mut tracker = StabilityTracker::new(Duration::ZERO, 2, n + 1);
                for p in &paths {
                    debouncer.push(p.clone(), t0);
                }
                let ready = debouncer.drain_ready(t0);
                for p in ready {
                    tracker.track(p, t0);
                }
                // Pass 1 & Pass 2 stability observations to transition all N items to Stable.
                let mut settled = 0usize;
                for pass in 0..2 {
                    let due = tracker.due(t0 + Duration::from_millis(pass));
                    for (idx, p) in due.iter().enumerate() {
                        if tracker
                            .observe(
                                p,
                                Observation::Present(facts_vec[idx].clone()),
                                t0 + Duration::from_millis(pass),
                            )
                            .is_some()
                        {
                            settled += 1;
                        }
                    }
                }
                black_box(settled);
            });
        });
    }
    group.finish();
}

/// BENCH-03: Pure rule evaluation throughput across 1k..100k FileFacts against 10 rules.
fn bench_rule_evaluation(c: &mut Criterion) {
    let dest = "/tmp/ff-bench-dest";
    let rules = bench_ruleset(dest);
    let clock = FixedClock::utc(BENCH_NOW_MS);
    let mut group = c.benchmark_group("bench_03_rule_eval");
    group.sampling_mode(SamplingMode::Flat);

    for &n in SCALES {
        let items = sample_rule_facts(n, "/tmp/ff-bench-src");
        group.sample_size(if n >= 50_000 { 10 } else { 30 });
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let mut action_count = 0usize;
                for facts in &items {
                    let outcome = evaluate_rules(black_box(&rules), black_box(facts), &clock);
                    action_count += outcome.actions.len();
                }
                black_box(action_count);
            });
        });
    }
    group.finish();
}

/// BENCH-04: SQLite query performance (`folder_summary`, `compute_dashboard_stats`,
/// `list_facts_for_folder`) at 1k..100k rows.
fn bench_db_queries(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");
    let clock = FixedClock::utc(BENCH_NOW_MS);
    let mut group = c.benchmark_group("bench_04_db_query");
    group.sampling_mode(SamplingMode::Flat);

    for &n in SCALES {
        let db = rt.block_on(async {
            let db = open_in_memory().await.expect("open in-memory db");
            repo_folders::insert(&db, "f-bench", "Downloads", "/tmp/ff-bench-db", true)
                .await
                .expect("insert benchmark folder");
            let records = sample_scanned_files(n, "/tmp/ff-bench-db");
            for chunk in records.chunks(2_000) {
                repo_files::upsert_batch(&db, "f-bench", chunk, BENCH_NOW_MS)
                    .await
                    .expect("seed file_records chunk");
            }
            db
        });

        group.sample_size(10);
        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(BenchmarkId::new("dashboard_stats", n), &n, |b, _| {
            b.to_async(&rt).iter(|| async {
                let stats = repo_stats::compute_dashboard_stats(&db, &clock)
                    .await
                    .expect("compute_dashboard_stats");
                black_box(stats.total_tracked_files);
            });
        });

        group.bench_with_input(BenchmarkId::new("folder_summary", n), &n, |b, _| {
            b.to_async(&rt).iter(|| async {
                let rows = repo_files::folder_summary(&db, "f-bench")
                    .await
                    .expect("folder_summary");
                black_box(rows.len());
            });
        });

        group.bench_with_input(BenchmarkId::new("list_facts_for_folder", n), &n, |b, _| {
            b.to_async(&rt).iter(|| async {
                let facts = repo_files::list_facts_for_folder(
                    &db,
                    "f-bench",
                    "/tmp/ff-bench-db",
                    "Downloads",
                )
                .await
                .expect("list_facts_for_folder");
                black_box(facts.len());
            });
        });
    }
    group.finish();
}

/// BENCH-05: Streaming BLAKE3 hashing speed (full 64 KiB buffered stream & partial 3-window).
fn bench_hasher(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("temp dir for hash bench");
    let mut group = c.benchmark_group("bench_05_blake3_hash");
    group.sampling_mode(SamplingMode::Flat);
    group.sample_size(15);

    for &size_mb in &[16u64, 64u64] {
        let total_bytes = size_mb * 1024 * 1024;
        let path = dir.path().join(format!("payload_{size_mb}mb.bin"));
        {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .expect("create hash payload file");
            let chunk = vec![0xA5u8; 1024 * 1024];
            for _ in 0..size_mb {
                f.write_all(&chunk).expect("write 1 MiB chunk");
            }
        }
        group.throughput(Throughput::Bytes(total_bytes));
        group.bench_with_input(
            BenchmarkId::new("full_hash_file", format!("{size_mb}MiB")),
            &path,
            |b, p| {
                b.iter(|| {
                    let h = full_hash_file(&RealFs, black_box(p)).expect("full_hash_file");
                    black_box(h);
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("partial_hash_file", format!("{size_mb}MiB")),
            &path,
            |b, p| {
                b.iter(|| {
                    let h = partial_hash_file(&RealFs, black_box(p), total_bytes)
                        .expect("partial_hash_file");
                    black_box(h);
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    folderflow_benches,
    bench_scan,
    bench_event_pipeline,
    bench_rule_evaluation,
    bench_db_queries,
    bench_hasher,
);
criterion_main!(folderflow_benches);
