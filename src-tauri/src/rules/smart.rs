//! Smart organizers implemented purely as configurable ordinary rule templates
//! (`P6-T04`, FR-120..FR-124).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::categories::{Category, OTHER};
use crate::error::{AppError, AppResult};
use crate::rules::models::{
    Action, Condition, ConditionNode, DateConditionOp, DateField, DateSource, ExecutionMode,
    RuleDraft, TextFieldKind, TextOp,
};

pub const DEFAULT_SCREENSHOT_PATTERNS: &[&str] =
    &["Screenshot*", "Screen Shot*", "CleanShot*", "Captura*"];

pub const DEFAULT_INVOICE_KEYWORDS: &[&str] =
    &["Invoice", "Receipt", "Bill", "Payment", "Tax", "GST"];

pub const ALLOWED_OLD_FILE_DAYS: &[u32] = &[7, 15, 30, 60, 90, 180, 365];

const SCREENSHOT_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "heic", "tiff"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SmartOrganizerSpec {
    Screenshots {
        #[serde(default)]
        extra_patterns: Vec<String>,
        #[serde(default)]
        destination: Option<String>,
        #[serde(default)]
        source_folders: Vec<String>,
    },
    Invoices {
        #[serde(default)]
        keywords: Vec<String>,
        #[serde(default)]
        destination: Option<String>,
        #[serde(default)]
        source_folders: Vec<String>,
    },
    DownloadCategory {
        category_id: String,
        #[serde(default)]
        destination: Option<String>,
        #[serde(default)]
        source_folders: Vec<String>,
    },
    OldFiles {
        days: u32,
        #[serde(default)]
        destination: Option<String>,
        #[serde(default)]
        source_folders: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SmartDownloadCategoryInfo {
    pub category_id: String,
    pub category_name: String,
    pub default_subpath: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SmartOrganizerCatalog {
    pub default_screenshot_patterns: Vec<String>,
    pub default_invoice_keywords: Vec<String>,
    pub allowed_old_file_days: Vec<u32>,
    pub download_categories: Vec<SmartDownloadCategoryInfo>,
}

fn default_subpath_for_category(cat: &Category) -> String {
    match cat.id.as_str() {
        "documents" => "Organized/Documents/{year}-{month}".into(),
        "images" => "Organized/Images/{year}".into(),
        "videos" => "Organized/Videos/{year}".into(),
        "audio" => "Organized/Audio/{year}".into(),
        "archives" => "Organized/Archives/{year}-{month}".into(),
        "installers" => "Organized/Installers".into(),
        _ => format!(
            "Organized/{}/{{year}}-{{month}}",
            cat.name.replace('/', "-")
        ),
    }
}

fn clean_destination(raw: Option<String>) -> Option<String> {
    raw.and_then(|s| {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_owned())
        }
    })
}

pub fn build_smart_catalog(categories: &[Category]) -> SmartOrganizerCatalog {
    let download_categories = categories
        .iter()
        .filter(|c| c.enabled && c.id != OTHER)
        .map(|c| SmartDownloadCategoryInfo {
            category_id: c.id.clone(),
            category_name: c.name.clone(),
            default_subpath: default_subpath_for_category(c),
        })
        .collect();

    SmartOrganizerCatalog {
        default_screenshot_patterns: DEFAULT_SCREENSHOT_PATTERNS
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
        default_invoice_keywords: DEFAULT_INVOICE_KEYWORDS
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
        allowed_old_file_days: ALLOWED_OLD_FILE_DAYS.to_vec(),
        download_categories,
    }
}

fn normalize_non_empty_list(
    raw: &[String],
    field: &str,
    reject_wildcardless_empty: bool,
) -> AppResult<Vec<String>> {
    let mut out = Vec::new();
    for item in raw {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            if reject_wildcardless_empty {
                return Err(AppError::validation(
                    field,
                    "Pattern/keyword cannot be empty.",
                ));
            }
            continue;
        }
        if !out
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(trimmed))
        {
            out.push(trimmed.to_owned());
        }
    }
    Ok(out)
}

pub fn build_smart_draft(
    spec: SmartOrganizerSpec,
    categories: &[Category],
) -> AppResult<RuleDraft> {
    match spec {
        SmartOrganizerSpec::Screenshots {
            extra_patterns,
            destination,
            source_folders,
        } => {
            let mut patterns: Vec<String> = DEFAULT_SCREENSHOT_PATTERNS
                .iter()
                .map(|s| (*s).to_owned())
                .collect();
            for p in normalize_non_empty_list(&extra_patterns, "extraPatterns", true)? {
                let normalized = if p.contains('*') || p.contains('?') {
                    p
                } else {
                    format!("*{p}*")
                };
                if !patterns.iter().any(|e| e.eq_ignore_ascii_case(&normalized)) {
                    patterns.push(normalized);
                }
            }
            let glob_nodes = patterns
                .into_iter()
                .map(|pattern| ConditionNode::Leaf {
                    condition: Condition::FilenameGlob {
                        pattern,
                        case_sensitive: false,
                    },
                })
                .collect();

            let dest = clean_destination(destination);
            let subpath_template = if dest.is_some() {
                "{year}-{month}".into()
            } else {
                "Screenshots/{year}-{month}".into()
            };

            Ok(RuleDraft {
                name: "Smart Screenshots Organizer".into(),
                description: Some(
                    "Sorts macOS screenshots and matching capture patterns into dated screenshot folders."
                        .into(),
                ),
                enabled: false,
                source_folders,
                conditions: ConditionNode::All {
                    children: vec![
                        ConditionNode::Any {
                            children: glob_nodes,
                        },
                        ConditionNode::Leaf {
                            condition: Condition::ExtensionGroup {
                                extensions: SCREENSHOT_EXTENSIONS
                                    .iter()
                                    .map(|s| (*s).to_owned())
                                    .collect(),
                            },
                        },
                    ],
                },
                actions: vec![Action::MoveIntoDynamicFolder {
                    root: dest,
                    subpath_template,
                }],
                date_source: DateSource::default(),
                execution_mode: ExecutionMode::FirstMatch,
            })
        }
        SmartOrganizerSpec::Invoices {
            keywords,
            destination,
            source_folders,
        } => {
            let cleaned = normalize_non_empty_list(&keywords, "keywords", true)?;
            let effective: Vec<String> = if cleaned.is_empty() {
                DEFAULT_INVOICE_KEYWORDS
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect()
            } else {
                cleaned
            };

            // FR-121: strictly filename-based keyword matching only.
            let children = effective
                .into_iter()
                .map(|kw| ConditionNode::Leaf {
                    condition: Condition::Text {
                        field: TextFieldKind::Name,
                        op: TextOp::Contains,
                        value: kw,
                        case_sensitive: false,
                    },
                })
                .collect();

            let dest = clean_destination(destination);
            let subpath_template = if dest.is_some() {
                "{year}/{month_name}".into()
            } else {
                "Invoices/{year}/{month_name}".into()
            };

            Ok(RuleDraft {
                name: "Smart Invoices & Receipts Organizer".into(),
                description: Some(
                    "Filename-only organizer for invoices, receipts, bills, payments, tax, and GST files."
                        .into(),
                ),
                enabled: false,
                source_folders,
                conditions: ConditionNode::Any { children },
                actions: vec![Action::MoveIntoDynamicFolder {
                    root: dest,
                    subpath_template,
                }],
                date_source: DateSource::default(),
                execution_mode: ExecutionMode::FirstMatch,
            })
        }
        SmartOrganizerSpec::DownloadCategory {
            category_id,
            destination,
            source_folders,
        } => {
            let cat = categories
                .iter()
                .find(|c| c.id == category_id && c.id != OTHER)
                .ok_or_else(|| {
                    AppError::validation(
                        "categoryId",
                        format!("Unknown or unsupported category \"{category_id}\"."),
                    )
                })?;
            let dest = clean_destination(destination);
            let subpath_template = if dest.is_some() {
                "{year}-{month}".into()
            } else {
                default_subpath_for_category(cat)
            };

            Ok(RuleDraft {
                name: format!("Downloads → {}", cat.name),
                description: Some(format!(
                    "Organize {} files from Downloads by month.",
                    cat.name
                )),
                enabled: false,
                source_folders,
                conditions: ConditionNode::Leaf {
                    condition: Condition::Category {
                        category_id: cat.id.clone(),
                        negate: false,
                    },
                },
                actions: vec![Action::MoveIntoDynamicFolder {
                    root: dest,
                    subpath_template,
                }],
                date_source: DateSource::default(),
                execution_mode: ExecutionMode::FirstMatch,
            })
        }
        SmartOrganizerSpec::OldFiles {
            days,
            destination,
            source_folders,
        } => {
            if !ALLOWED_OLD_FILE_DAYS.contains(&days) {
                return Err(AppError::validation(
                    "days",
                    "Old-file archive threshold must be one of 7, 15, 30, 60, 90, 180, or 365 days.",
                ));
            }
            let dest = clean_destination(destination);
            let subpath_template = if dest.is_some() {
                format!("{days}-days/{{year}}-{{month}}")
            } else {
                format!("Archive/{days}-days/{{year}}-{{month}}")
            };

            Ok(RuleDraft {
                name: format!("Archive files older than {days} days"),
                description: Some(format!(
                    "Safely archive files untouched for {days} days. Never deletes files."
                )),
                enabled: false,
                source_folders,
                conditions: ConditionNode::Leaf {
                    condition: Condition::Date {
                        field: DateField::Modified,
                        op: DateConditionOp::OlderThanDays { days },
                    },
                },
                actions: vec![Action::MoveIntoDynamicFolder {
                    root: dest,
                    subpath_template,
                }],
                date_source: DateSource::default(),
                execution_mode: ExecutionMode::FirstMatch,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::categories::CategoryService;
    use crate::engine::paths::RootPolicy;
    use crate::rules::evaluator::evaluate_rules;
    use crate::rules::models::{Action, Condition, Rule, RuleOrigin, TextFieldKind};
    use crate::rules::service::RuleService;
    use crate::test_support::tempdir;
    use crate::utils::clock::FixedClock;

    fn sample_fact(
        folder: &str,
        name: &str,
        category: &str,
        modified_ms: i64,
    ) -> crate::rules::models::FileFacts {
        let (stem, extension) = crate::rules::models::FileFacts::split_name(name);
        crate::rules::models::FileFacts {
            path: format!("{folder}/{name}"),
            name: name.into(),
            stem,
            extension,
            mime: None,
            category: category.into(),
            size: 1024,
            created_ms: Some(modified_ms),
            modified_ms,
            added_ms: Some(modified_ms),
            is_hidden: false,
            is_duplicate: false,
            content_hash: None,
            source_folder_id: "f1".into(),
            source_folder_path: folder.into(),
            source_folder_name: "Downloads".into(),
        }
    }

    fn draft_to_enabled_rule(id: &str, draft: RuleDraft) -> Rule {
        Rule {
            id: id.into(),
            name: draft.name,
            description: draft.description,
            enabled: true,
            priority: 1,
            source_folders: draft.source_folders,
            conditions: draft.conditions,
            actions: draft.actions,
            date_source: draft.date_source,
            execution_mode: draft.execution_mode,
            origin: RuleOrigin::Preset,
            last_run_at: None,
            stat_matched: 0,
            stat_succeeded: 0,
            stat_failed: 0,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn has_only_name_conditions(node: &ConditionNode) -> bool {
        match node {
            ConditionNode::All { children } | ConditionNode::Any { children } => {
                children.iter().all(has_only_name_conditions)
            }
            ConditionNode::Not { child } => has_only_name_conditions(child),
            ConditionNode::Leaf { condition } => matches!(
                condition,
                Condition::Text {
                    field: TextFieldKind::Name,
                    ..
                }
            ),
        }
    }

    #[tokio::test]
    async fn r_smart_01_screenshot_organizer_supports_default_and_custom_patterns() {
        let db = crate::database::open_in_memory().await.expect("db");
        let cat_svc = CategoryService::new(db.clone());
        cat_svc.ensure_builtins().await.expect("builtins");
        let cats = cat_svc.list().await.expect("cats");

        let draft = build_smart_draft(
            SmartOrganizerSpec::Screenshots {
                extra_patterns: vec!["Monosnap*".into(), "FrameCapture".into()],
                destination: None,
                source_folders: Vec::new(),
            },
            &cats,
        )
        .expect("screenshot draft");
        assert!(!draft.enabled, "smart presets start disabled");

        let rule = draft_to_enabled_rule("r-shot", draft);
        let clock = FixedClock::utc(1_770_000_000_000);

        let mac_shot = sample_fact(
            "/Users/u/Desktop",
            "Screenshot 2026-01-02.png",
            "images",
            1_769_000_000_000,
        );
        let cleanshot = sample_fact(
            "/Users/u/Desktop",
            "CleanShot 2026.png",
            "images",
            1_769_000_000_000,
        );
        let custom_glob = sample_fact(
            "/Users/u/Desktop",
            "Monosnap_clip.jpg",
            "images",
            1_769_000_000_000,
        );
        let custom_word = sample_fact(
            "/Users/u/Desktop",
            "my-FrameCapture-1.png",
            "images",
            1_769_000_000_000,
        );
        let non_img = sample_fact(
            "/Users/u/Desktop",
            "Screenshot notes.txt",
            "documents",
            1_769_000_000_000,
        );

        assert!(
            !evaluate_rules(std::slice::from_ref(&rule), &mac_shot, &clock)
                .matched_rules
                .is_empty()
        );
        assert!(
            !evaluate_rules(std::slice::from_ref(&rule), &cleanshot, &clock)
                .matched_rules
                .is_empty()
        );
        assert!(
            !evaluate_rules(std::slice::from_ref(&rule), &custom_glob, &clock)
                .matched_rules
                .is_empty()
        );
        assert!(
            !evaluate_rules(std::slice::from_ref(&rule), &custom_word, &clock)
                .matched_rules
                .is_empty()
        );
        assert!(evaluate_rules(&[rule], &non_img, &clock)
            .matched_rules
            .is_empty());
    }

    #[tokio::test]
    async fn r_smart_02_invoice_organizer_is_strictly_filename_based_with_custom_keywords() {
        let db = crate::database::open_in_memory().await.expect("db");
        let cat_svc = CategoryService::new(db.clone());
        cat_svc.ensure_builtins().await.expect("builtins");
        let cats = cat_svc.list().await.expect("cats");

        let default_draft = build_smart_draft(
            SmartOrganizerSpec::Invoices {
                keywords: Vec::new(),
                destination: None,
                source_folders: Vec::new(),
            },
            &cats,
        )
        .expect("default invoice");
        assert!(has_only_name_conditions(&default_draft.conditions));
        assert!(!default_draft.enabled);

        let custom_draft = build_smart_draft(
            SmartOrganizerSpec::Invoices {
                keywords: vec!["Faktura".into(), "GST".into()],
                destination: None,
                source_folders: Vec::new(),
            },
            &cats,
        )
        .expect("custom invoice");
        assert!(has_only_name_conditions(&custom_draft.conditions));

        let rule = draft_to_enabled_rule("r-inv", custom_draft);
        let clock = FixedClock::utc(1_770_000_000_000);

        let yes = sample_fact(
            "/Users/u/Downloads",
            "Q1_faktura_2026.pdf",
            "documents",
            1_769_000_000_000,
        );
        let no = sample_fact(
            "/Users/u/Downloads",
            "holiday_photo.jpg",
            "images",
            1_769_000_000_000,
        );
        assert!(!evaluate_rules(std::slice::from_ref(&rule), &yes, &clock)
            .matched_rules
            .is_empty());
        assert!(evaluate_rules(&[rule], &no, &clock)
            .matched_rules
            .is_empty());
    }

    #[tokio::test]
    async fn r_smart_03_download_presets_per_category_catalog_and_draft() {
        let db = crate::database::open_in_memory().await.expect("db");
        let cat_svc = CategoryService::new(db.clone());
        cat_svc.ensure_builtins().await.expect("builtins");
        let cats = cat_svc.list().await.expect("cats");

        let catalog = build_smart_catalog(&cats);
        assert!(catalog
            .download_categories
            .iter()
            .any(|c| c.category_id == "archives"));
        assert!(catalog
            .download_categories
            .iter()
            .any(|c| c.category_id == "documents"));

        let arch = build_smart_draft(
            SmartOrganizerSpec::DownloadCategory {
                category_id: "archives".into(),
                destination: None,
                source_folders: Vec::new(),
            },
            &cats,
        )
        .expect("archives preset");
        assert!(!arch.enabled);

        let rule = draft_to_enabled_rule("r-dl", arch);
        let clock = FixedClock::utc(1_770_000_000_000);

        let zip = sample_fact(
            "/Users/u/Downloads",
            "release.zip",
            "archives",
            1_769_000_000_000,
        );
        let doc = sample_fact(
            "/Users/u/Downloads",
            "notes.pdf",
            "documents",
            1_769_000_000_000,
        );
        assert!(!evaluate_rules(std::slice::from_ref(&rule), &zip, &clock)
            .matched_rules
            .is_empty());
        assert!(evaluate_rules(&[rule], &doc, &clock)
            .matched_rules
            .is_empty());

        // Reject unknown category
        assert!(build_smart_draft(
            SmartOrganizerSpec::DownloadCategory {
                category_id: "does-not-exist".into(),
                destination: None,
                source_folders: Vec::new(),
            },
            &cats,
        )
        .is_err());
    }

    #[tokio::test]
    async fn r_smart_04_old_file_organizer_enforces_thresholds_never_deletes_and_persists_disabled()
    {
        let (_d, root) = tempdir();
        let db = crate::database::open_in_memory().await.expect("db");
        let cat_svc = CategoryService::new(db.clone());
        cat_svc.ensure_builtins().await.expect("builtins");
        let cats = cat_svc.list().await.expect("cats");

        // Reject disallowed threshold values
        for bad in [0u32, 1, 14, 45, 400] {
            assert!(
                build_smart_draft(
                    SmartOrganizerSpec::OldFiles {
                        days: bad,
                        destination: None,
                        source_folders: Vec::new(),
                    },
                    &cats,
                )
                .is_err(),
                "threshold {bad} days should be rejected"
            );
        }

        // All 7 allowed thresholds build valid archive rules that never delete
        for days in ALLOWED_OLD_FILE_DAYS {
            let draft = build_smart_draft(
                SmartOrganizerSpec::OldFiles {
                    days: *days,
                    destination: None,
                    source_folders: Vec::new(),
                },
                &cats,
            )
            .expect("valid days");
            assert!(!draft.enabled);
            assert!(draft.actions.iter().all(|a| matches!(
                a,
                Action::Move { .. } | Action::MoveIntoDynamicFolder { .. }
            )));
        }

        // Persisting via RuleService stores an ordinary disabled Preset rule evaluated normally
        let svc = RuleService::new(
            db,
            RootPolicy {
                home: root.clone(),
                app_data_dir: root.join("AppData"),
            },
        );
        let saved = svc
            .create_smart_preset(SmartOrganizerSpec::OldFiles {
                days: 90,
                destination: None,
                source_folders: Vec::new(),
            })
            .await
            .expect("create_smart_preset");
        assert!(!saved.enabled, "persisted smart preset must start disabled");
        assert_eq!(saved.origin, RuleOrigin::Preset);

        let now_ms = 1_770_000_000_000i64;
        let old_ms = now_ms - (95 * 86_400_000);
        let fresh_ms = now_ms - (10 * 86_400_000);
        let mut enabled_saved = saved.clone();
        enabled_saved.enabled = true;
        let clock = FixedClock::utc(now_ms);

        let old_file = sample_fact("/Users/u/Downloads", "stale.zip", "archives", old_ms);
        let fresh_file = sample_fact("/Users/u/Downloads", "fresh.zip", "archives", fresh_ms);
        assert!(!evaluate_rules(&[enabled_saved.clone()], &old_file, &clock)
            .matched_rules
            .is_empty());
        assert!(evaluate_rules(&[enabled_saved], &fresh_file, &clock)
            .matched_rules
            .is_empty());
    }

    #[test]
    fn edge_06_smart_rules_match_unicode_emoji_double_extension_leading_dot_and_255b_names() {
        let cats = crate::categories::builtin_categories();
        let clock = FixedClock::utc(1_770_000_000_000);

        let shot_draft = build_smart_draft(
            SmartOrganizerSpec::Screenshots {
                extra_patterns: Vec::new(),
                destination: None,
                source_folders: Vec::new(),
            },
            &cats,
        )
        .expect("screenshot");
        let shot_rule = draft_to_enabled_rule("r-shot-edge", shot_draft);

        let shot_archive = sample_fact(
            "/Users/u/Desktop",
            "Screenshot 2026 🎉 Caf\u{00e9}.tar.gz",
            "archives",
            1_769_900_000_000,
        );
        assert!(
            evaluate_rules(std::slice::from_ref(&shot_rule), &shot_archive, &clock)
                .matched_rules
                .is_empty(),
            "non-image double-extension .tar.gz must not match screenshot rule"
        );

        let shot_fact = sample_fact(
            "/Users/u/Desktop",
            "Screenshot 2026 🎉 Caf\u{00e9}.v2.png",
            "images",
            1_769_900_000_000,
        );
        assert!(
            !evaluate_rules(std::slice::from_ref(&shot_rule), &shot_fact, &clock)
                .matched_rules
                .is_empty(),
            "screenshot matcher should handle Unicode/emoji/spaces/dotted stem + .png"
        );

        let dl_draft = build_smart_draft(
            SmartOrganizerSpec::DownloadCategory {
                category_id: "archives".into(),
                destination: None,
                source_folders: Vec::new(),
            },
            &cats,
        )
        .expect("download category");
        let dl_rule = draft_to_enabled_rule("r-dl-edge", dl_draft);
        let double_ext_fact = sample_fact(
            "/Users/u/Downloads",
            "Caf\u{00e9} 🎉 bundle.tar.gz",
            "archives",
            1_769_900_000_000,
        );
        assert!(
            !evaluate_rules(&[dl_rule], &double_ext_fact, &clock)
                .matched_rules
                .is_empty(),
            "category smart rule should match Unicode/emoji/.tar.gz archive"
        );

        let long_non_shot = format!("{}.png", "x".repeat(251));
        assert_eq!(long_non_shot.len(), 255);
        let long_fact = sample_fact(
            "/Users/u/Desktop",
            &long_non_shot,
            "images",
            1_769_900_000_000,
        );
        assert!(evaluate_rules(&[shot_rule], &long_fact, &clock)
            .matched_rules
            .is_empty());

        let inv_draft = build_smart_draft(
            SmartOrganizerSpec::Invoices {
                keywords: Vec::new(),
                destination: None,
                source_folders: Vec::new(),
            },
            &cats,
        )
        .expect("invoice");
        let inv_rule = draft_to_enabled_rule("r-inv-edge", inv_draft);
        let tricky_inv = sample_fact(
            "/Users/u/Downloads",
            ".env.invoice \"2026\", 🎉.pdf",
            "documents",
            1_769_900_000_000,
        );
        assert!(!evaluate_rules(&[inv_rule], &tricky_inv, &clock)
            .matched_rules
            .is_empty());
    }
}
