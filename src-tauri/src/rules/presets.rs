//! Starter / smart rule templates (P3-T08; FR-005, FR-120–124).
//!
//! All starter rules are ordinary `RuleDraft` definitions with `enabled: false`
//! and `origin: RuleOrigin::Preset` (no special engine code paths).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::rules::models::{
    Action, Condition, ConditionNode, DateConditionOp, DateField, DateSource, ExecutionMode,
    RuleDraft, TextFieldKind, TextOp,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RuleTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category_hint: String,
    pub draft: RuleDraft,
}

fn screenshot_globs() -> ConditionNode {
    ConditionNode::Any {
        children: vec![
            ConditionNode::Leaf {
                condition: Condition::FilenameGlob {
                    pattern: "Screenshot*".into(),
                    case_sensitive: false,
                },
            },
            ConditionNode::Leaf {
                condition: Condition::FilenameGlob {
                    pattern: "Screen Shot*".into(),
                    case_sensitive: false,
                },
            },
            ConditionNode::Leaf {
                condition: Condition::FilenameGlob {
                    pattern: "CleanShot*".into(),
                    case_sensitive: false,
                },
            },
        ],
    }
}

pub fn starter_templates() -> Vec<RuleTemplate> {
    vec![
        RuleTemplate {
            id: "invoices".into(),
            name: "Invoices & Receipts".into(),
            description: "Sort invoices, receipts, bills, tax and GST files into Invoices by year and month.".into(),
            category_hint: "documents".into(),
            draft: RuleDraft {
                name: "Invoices & Receipts".into(),
                description: Some("Sort invoices, receipts, bills, tax and GST files by year and month.".into()),
                enabled: false,
                source_folders: Vec::new(),
                conditions: ConditionNode::All {
                    children: vec![
                        ConditionNode::Leaf {
                            condition: Condition::ExtensionGroup {
                                extensions: vec![
                                    "pdf".into(),
                                    "png".into(),
                                    "jpg".into(),
                                    "jpeg".into(),
                                ],
                            },
                        },
                        ConditionNode::Any {
                            children: ["invoice", "receipt", "bill", "payment", "tax", "gst"]
                                .into_iter()
                                .map(|kw| ConditionNode::Leaf {
                                    condition: Condition::Text {
                                        field: TextFieldKind::Name,
                                        op: TextOp::Contains,
                                        value: kw.into(),
                                        case_sensitive: false,
                                    },
                                })
                                .collect(),
                        },
                    ],
                },
                actions: vec![Action::MoveIntoDynamicFolder {
                    root: None,
                    subpath_template: "Invoices/{year}/{month_name}".into(),
                }],
                date_source: DateSource::Modified,
                execution_mode: ExecutionMode::FirstMatch,
            },
        },
        RuleTemplate {
            id: "screenshots".into(),
            name: "Screenshots".into(),
            description: "Organize macOS and CleanShot screenshots into a Screenshots folder by year and month.".into(),
            category_hint: "images".into(),
            draft: RuleDraft {
                name: "Screenshots".into(),
                description: Some("Organize macOS and CleanShot screenshots by year and month.".into()),
                enabled: false,
                source_folders: Vec::new(),
                conditions: ConditionNode::All {
                    children: vec![
                        ConditionNode::Leaf {
                            condition: Condition::Category {
                                category_id: "images".into(),
                                negate: false,
                            },
                        },
                        screenshot_globs(),
                    ],
                },
                actions: vec![Action::MoveIntoDynamicFolder {
                    root: None,
                    subpath_template: "Screenshots/{year}-{month}".into(),
                }],
                date_source: DateSource::Modified,
                execution_mode: ExecutionMode::FirstMatch,
            },
        },
        RuleTemplate {
            id: "pdfs".into(),
            name: "PDF Documents".into(),
            description: "Move PDF files into a PDFs folder organized by year.".into(),
            category_hint: "documents".into(),
            draft: RuleDraft {
                name: "PDF Documents".into(),
                description: Some("Move PDF files into a PDFs folder organized by year.".into()),
                enabled: false,
                source_folders: Vec::new(),
                conditions: ConditionNode::All {
                    children: vec![ConditionNode::Leaf {
                        condition: Condition::ExtensionGroup {
                            extensions: vec!["pdf".into()],
                        },
                    }],
                },
                actions: vec![Action::MoveIntoDynamicFolder {
                    root: None,
                    subpath_template: "PDFs/{year}".into(),
                }],
                date_source: DateSource::Modified,
                execution_mode: ExecutionMode::FirstMatch,
            },
        },
        RuleTemplate {
            id: "images".into(),
            name: "Images & Photos".into(),
            description: "Organize photos and images (excluding screenshots) by year and month.".into(),
            category_hint: "images".into(),
            draft: RuleDraft {
                name: "Images & Photos".into(),
                description: Some("Organize photos and images (excluding screenshots) by year and month.".into()),
                enabled: false,
                source_folders: Vec::new(),
                conditions: ConditionNode::All {
                    children: vec![
                        ConditionNode::Leaf {
                            condition: Condition::Category {
                                category_id: "images".into(),
                                negate: false,
                            },
                        },
                        ConditionNode::Not {
                            child: Box::new(screenshot_globs()),
                        },
                    ],
                },
                actions: vec![Action::MoveIntoDynamicFolder {
                    root: None,
                    subpath_template: "Images/{year}/{month_name}".into(),
                }],
                date_source: DateSource::Modified,
                execution_mode: ExecutionMode::FirstMatch,
            },
        },
        RuleTemplate {
            id: "videos".into(),
            name: "Videos".into(),
            description: "Move video files into a Videos folder organized by year.".into(),
            category_hint: "videos".into(),
            draft: RuleDraft {
                name: "Videos".into(),
                description: Some("Move video files into a Videos folder organized by year.".into()),
                enabled: false,
                source_folders: Vec::new(),
                conditions: ConditionNode::All {
                    children: vec![ConditionNode::Leaf {
                        condition: Condition::Category {
                            category_id: "videos".into(),
                            negate: false,
                        },
                    }],
                },
                actions: vec![Action::MoveIntoDynamicFolder {
                    root: None,
                    subpath_template: "Videos/{year}".into(),
                }],
                date_source: DateSource::Modified,
                execution_mode: ExecutionMode::FirstMatch,
            },
        },
        RuleTemplate {
            id: "archives".into(),
            name: "Archives".into(),
            description: "Move compressed archives into an Archives folder by year and month.".into(),
            category_hint: "archives".into(),
            draft: RuleDraft {
                name: "Archives".into(),
                description: Some("Move compressed archives into an Archives folder by year and month.".into()),
                enabled: false,
                source_folders: Vec::new(),
                conditions: ConditionNode::All {
                    children: vec![ConditionNode::Leaf {
                        condition: Condition::Category {
                            category_id: "archives".into(),
                            negate: false,
                        },
                    }],
                },
                actions: vec![Action::MoveIntoDynamicFolder {
                    root: None,
                    subpath_template: "Archives/{year}-{month}".into(),
                }],
                date_source: DateSource::Modified,
                execution_mode: ExecutionMode::FirstMatch,
            },
        },
        RuleTemplate {
            id: "old_downloads".into(),
            name: "Downloads Older Than 30 Days".into(),
            description: "Move files modified more than 30 days ago into an Archive folder by year and month.".into(),
            category_hint: "other".into(),
            draft: RuleDraft {
                name: "Downloads Older Than 30 Days".into(),
                description: Some("Move files modified more than 30 days ago into an Archive folder.".into()),
                enabled: false,
                source_folders: Vec::new(),
                conditions: ConditionNode::All {
                    children: vec![ConditionNode::Leaf {
                        condition: Condition::Date {
                            field: DateField::Modified,
                            op: DateConditionOp::OlderThanDays { days: 30 },
                        },
                    }],
                },
                actions: vec![Action::MoveIntoDynamicFolder {
                    root: None,
                    subpath_template: "Archive/{year}-{month}".into(),
                }],
                date_source: DateSource::Modified,
                execution_mode: ExecutionMode::FirstMatch,
            },
        },
    ]
}

pub fn find_starter_template(id: &str) -> Option<RuleTemplate> {
    starter_templates().into_iter().find(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::conditions::evaluate_node;
    use crate::rules::models::FileFacts;
    use crate::rules::validate::{validate_rule_draft, ValidationContext, ValidationSeverity};
    use crate::utils::clock::FixedClock;
    use chrono::TimeZone;

    fn utc_ms(y: i32, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> i64 {
        chrono::Utc
            .with_ymd_and_hms(y, m, d, hh, mm, ss)
            .single()
            .expect("valid utc datetime")
            .timestamp_millis()
    }

    fn make_facts(name: &str, category: &str, modified_ms: i64) -> FileFacts {
        let (stem, extension) = FileFacts::split_name(name);
        FileFacts {
            path: format!("/Users/alice/Downloads/{name}"),
            name: name.to_owned(),
            stem,
            extension,
            mime: None,
            category: category.to_owned(),
            size: 40_000,
            created_ms: Some(modified_ms),
            modified_ms,
            added_ms: Some(modified_ms),
            is_hidden: false,
            is_duplicate: false,
            content_hash: None,
            source_folder_id: "f-dl".into(),
            source_folder_path: "/Users/alice/Downloads".into(),
            source_folder_name: "Downloads".into(),
        }
    }

    #[test]
    fn r_tmpl_01_all_seven_starter_templates_exist_are_disabled_and_validate_cleanly() {
        let templates = starter_templates();
        let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
        for expected in [
            "pdfs",
            "screenshots",
            "images",
            "videos",
            "archives",
            "invoices",
            "old_downloads",
        ] {
            assert!(
                ids.contains(&expected),
                "missing starter template '{expected}', got: {ids:?}"
            );
        }

        for t in &templates {
            assert!(
                !t.draft.enabled,
                "starter template '{}' must be disabled by default",
                t.id
            );
            let issues = validate_rule_draft(&t.draft, &ValidationContext::default());
            let errors: Vec<_> = issues
                .into_iter()
                .filter(|i| i.severity == ValidationSeverity::Error)
                .collect();
            assert!(
                errors.is_empty(),
                "starter template '{}' failed validation: {errors:?}",
                t.id
            );
        }
    }

    #[test]
    fn r_tmpl_02_screenshots_and_images_templates_partition_image_files_correctly() {
        let now = utc_ms(2026, 11, 14, 12, 0, 0);
        let clock = FixedClock::utc(now);

        let screenshots = find_starter_template("screenshots").expect("screenshots template");
        let images = find_starter_template("images").expect("images template");

        for shot_name in [
            "Screenshot 2026-11-14 at 10.05.22.png",
            "Screen Shot 2026-11-14 at 9.00.00 AM.png",
            "CleanShot 2026-11-14 at 10.12.00.png",
        ] {
            let f = make_facts(shot_name, "images", now);
            assert!(
                evaluate_node(&screenshots.draft.conditions, &f, &clock).matched,
                "screenshots preset should match {shot_name}"
            );
            assert!(
                !evaluate_node(&images.draft.conditions, &f, &clock).matched,
                "images preset should exclude screenshot {shot_name}"
            );
        }

        let photo = make_facts("IMG_4021.heic", "images", now);
        assert!(!evaluate_node(&screenshots.draft.conditions, &photo, &clock).matched);
        assert!(evaluate_node(&images.draft.conditions, &photo, &clock).matched);
    }

    #[test]
    fn r_tmpl_03_invoices_pdfs_videos_archives_and_old_downloads_match_targets() {
        let now = utc_ms(2026, 11, 14, 12, 0, 0);
        let clock = FixedClock::utc(now);

        let invoices = find_starter_template("invoices").expect("invoices");
        let pdfs = find_starter_template("pdfs").expect("pdfs");
        let videos = find_starter_template("videos").expect("videos");
        let archives = find_starter_template("archives").expect("archives");
        let old_dl = find_starter_template("old_downloads").expect("old_downloads");

        for inv in [
            "Acme_Invoice_2026.pdf",
            "Uber_Receipt_Nov.pdf",
            "Electric_Bill.png",
            "GST_Payment_2026.jpg",
            "Tax_Statement.pdf",
        ] {
            let f = make_facts(inv, "documents", now);
            assert!(
                evaluate_node(&invoices.draft.conditions, &f, &clock).matched,
                "invoices preset should match {inv}"
            );
        }
        let regular_pdf = make_facts("User_Manual.pdf", "documents", now);
        assert!(!evaluate_node(&invoices.draft.conditions, &regular_pdf, &clock).matched);
        assert!(evaluate_node(&pdfs.draft.conditions, &regular_pdf, &clock).matched);

        let video = make_facts("Demo.mp4", "videos", now);
        assert!(evaluate_node(&videos.draft.conditions, &video, &clock).matched);

        let archive = make_facts("Bundle.zip", "archives", now);
        assert!(evaluate_node(&archives.draft.conditions, &archive, &clock).matched);

        let old_file = make_facts("OldInstaller.dmg", "installers", now - 35 * 86_400_000);
        let fresh_file = make_facts("NewInstaller.dmg", "installers", now - 5 * 86_400_000);
        assert!(evaluate_node(&old_dl.draft.conditions, &old_file, &clock).matched);
        assert!(!evaluate_node(&old_dl.draft.conditions, &fresh_file, &clock).matched);
    }
}
