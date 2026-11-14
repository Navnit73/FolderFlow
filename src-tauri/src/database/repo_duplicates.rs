//! Persistence for duplicate groups and their file members (`P6-T02`, FR-113..115).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::Row;

use super::Db;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateMemberView {
    pub file_record_id: String,
    pub folder_id: String,
    pub path: String,
    pub filename: String,
    pub extension: Option<String>,
    #[specta(type = f64)]
    pub size: u64,
    #[specta(type = f64)]
    pub created_ms: i64,
    #[specta(type = f64)]
    pub modified_ms: i64,
    pub is_keeper: bool,
    pub exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroupSummary {
    pub id: String,
    pub content_hash: String,
    #[specta(type = f64)]
    pub file_size: u64,
    pub ignored: bool,
    pub copy_count: u32,
    #[specta(type = f64)]
    pub wasted_bytes: u64,
    pub all_same_name: bool,
    pub same_content_different_names: bool,
    #[specta(type = f64)]
    pub created_at: i64,
    pub members: Vec<DuplicateMemberView>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateSummaryTotals {
    pub total_groups: u32,
    pub active_groups: u32,
    pub ignored_groups: u32,
    pub total_duplicate_copies: u32,
    #[specta(type = f64)]
    pub total_wasted_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroupsResponse {
    pub totals: DuplicateSummaryTotals,
    pub groups: Vec<DuplicateGroupSummary>,
}

/// Candidate member passed from [`crate::duplicates::finder`] into persistence.
#[derive(Debug, Clone)]
pub struct NewDuplicateMember {
    pub file_record_id: String,
    pub path: String,
    pub modified_ns: i64,
    pub created_ns: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct NewDuplicateGroup {
    pub content_hash: String,
    pub file_size: u64,
    pub members: Vec<NewDuplicateMember>,
}

/// Atomically reconciles `duplicate_groups` and `duplicate_members` from a full scan,
/// preserving user `ignored` flags and surviving `is_keeper` choices across rescans.
pub async fn replace_groups_from_scan(
    db: &Db,
    groups: &[NewDuplicateGroup],
    scoped_folder_ids: Option<&[String]>,
    now_ms: i64,
) -> AppResult<()> {
    if scoped_folder_ids.is_some_and(<[String]>::is_empty) {
        return Ok(());
    }

    let mut tx = db.begin().await?;

    let existing_ignored: HashMap<String, bool> =
        sqlx::query("SELECT content_hash, ignored FROM duplicate_groups")
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .map(|r| {
                let h: String = r.get("content_hash");
                let ig: i64 = r.get("ignored");
                (h, ig != 0)
            })
            .collect();

    let existing_keepers: HashSet<(String, String)> = sqlx::query(
        "SELECT dg.content_hash, dm.file_record_id \
         FROM duplicate_members dm \
         JOIN duplicate_groups dg ON dg.id = dm.group_id \
         WHERE dm.is_keeper = 1",
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|r| (r.get("content_hash"), r.get("file_record_id")))
    .collect();

    let mut surviving_by_hash: HashMap<String, String> = HashMap::new();
    match scoped_folder_ids {
        None => {
            sqlx::query("DELETE FROM duplicate_groups")
                .execute(&mut *tx)
                .await?;
        }
        Some(ids) => {
            let scope_set: HashSet<&str> = ids.iter().map(String::as_str).collect();
            let member_rows = sqlx::query(
                "SELECT dg.id AS group_id, dg.content_hash, dm.file_record_id, fr.folder_id \
                 FROM duplicate_groups dg \
                 LEFT JOIN duplicate_members dm ON dm.group_id = dg.id \
                 LEFT JOIN file_records fr ON fr.id = dm.file_record_id",
            )
            .fetch_all(&mut *tx)
            .await?;

            struct GroupScopeInfo {
                content_hash: String,
                in_scope_records: Vec<String>,
                has_out_of_scope: bool,
            }
            let mut by_group: HashMap<String, GroupScopeInfo> = HashMap::new();
            for r in member_rows {
                let gid: String = r.get("group_id");
                let content_hash: String = r.get("content_hash");
                let entry = by_group.entry(gid).or_insert_with(|| GroupScopeInfo {
                    content_hash,
                    in_scope_records: Vec::new(),
                    has_out_of_scope: false,
                });
                let fid: Option<String> = r.get("folder_id");
                let frid: Option<String> = r.get("file_record_id");
                match (fid, frid) {
                    (Some(folder_id), Some(file_record_id))
                        if scope_set.contains(folder_id.as_str()) =>
                    {
                        entry.in_scope_records.push(file_record_id);
                    }
                    (Some(_), Some(_)) => {
                        entry.has_out_of_scope = true;
                    }
                    _ => {}
                }
            }

            for (gid, info) in by_group {
                if !info.has_out_of_scope {
                    sqlx::query("DELETE FROM duplicate_groups WHERE id = ?1")
                        .bind(&gid)
                        .execute(&mut *tx)
                        .await?;
                } else {
                    for frid in &info.in_scope_records {
                        sqlx::query(
                            "DELETE FROM duplicate_members WHERE group_id = ?1 AND file_record_id = ?2",
                        )
                        .bind(&gid)
                        .bind(frid)
                        .execute(&mut *tx)
                        .await?;
                    }
                    surviving_by_hash.insert(info.content_hash, gid);
                }
            }
        }
    }

    for g in groups {
        if let Some(existing_gid) = surviving_by_hash.get(&g.content_hash) {
            for m in &g.members {
                let is_keeper =
                    existing_keepers.contains(&(g.content_hash.clone(), m.file_record_id.clone()));
                sqlx::query(
                    "INSERT OR IGNORE INTO duplicate_members (group_id, file_record_id, is_keeper) \
                     VALUES (?1, ?2, ?3)",
                )
                .bind(existing_gid)
                .bind(&m.file_record_id)
                .bind(if is_keeper { 1i64 } else { 0i64 })
                .execute(&mut *tx)
                .await?;
            }
            let keeper_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM duplicate_members WHERE group_id = ?1 AND is_keeper = 1",
            )
            .bind(existing_gid)
            .fetch_one(&mut *tx)
            .await?;
            if keeper_count == 0 {
                sqlx::query(
                    "UPDATE duplicate_members SET is_keeper = 1 \
                     WHERE group_id = ?1 AND file_record_id = (\
                         SELECT file_record_id FROM duplicate_members WHERE group_id = ?1 LIMIT 1\
                     )",
                )
                .bind(existing_gid)
                .execute(&mut *tx)
                .await?;
            }
            continue;
        }
        if g.members.len() < 2 {
            continue;
        }
        let group_id = uuid::Uuid::now_v7().to_string();
        let ignored = existing_ignored
            .get(&g.content_hash)
            .copied()
            .unwrap_or(false);
        let size_i = i64::try_from(g.file_size).unwrap_or(i64::MAX);

        sqlx::query(
            "INSERT INTO duplicate_groups (id, content_hash, file_size, ignored, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(&group_id)
        .bind(&g.content_hash)
        .bind(size_i)
        .bind(if ignored { 1i64 } else { 0i64 })
        .bind(now_ms)
        .execute(&mut *tx)
        .await?;

        // Elect exactly one keeper: preserve prior surviving keeper, else oldest/shortest path.
        let preserved = g
            .members
            .iter()
            .find(|m| {
                existing_keepers.contains(&(g.content_hash.clone(), m.file_record_id.clone()))
            })
            .map(|m| m.file_record_id.as_str());

        let default_keeper = if preserved.is_none() {
            g.members
                .iter()
                .min_by(|a, b| {
                    let a_ts = a.created_ns.unwrap_or(a.modified_ns);
                    let b_ts = b.created_ns.unwrap_or(b.modified_ns);
                    a_ts.cmp(&b_ts)
                        .then_with(|| a.path.len().cmp(&b.path.len()))
                        .then_with(|| a.path.cmp(&b.path))
                })
                .map(|m| m.file_record_id.as_str())
        } else {
            None
        };
        let keeper_id = preserved.or(default_keeper);

        for m in &g.members {
            let is_keeper = keeper_id == Some(m.file_record_id.as_str());
            sqlx::query(
                "INSERT INTO duplicate_members (group_id, file_record_id, is_keeper) \
                 VALUES (?1, ?2, ?3)",
            )
            .bind(&group_id)
            .bind(&m.file_record_id)
            .bind(if is_keeper { 1i64 } else { 0i64 })
            .execute(&mut *tx)
            .await?;
        }
    }

    if scoped_folder_ids.is_some() {
        sqlx::query(
            "DELETE FROM duplicate_groups \
             WHERE (SELECT COUNT(*) FROM duplicate_members WHERE group_id = duplicate_groups.id) < 2",
        )
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// Lists all persisted duplicate groups with summary totals, sorted by wasted bytes descending.
pub async fn list_groups(
    db: &Db,
    include_ignored: bool,
    search: Option<&str>,
) -> AppResult<DuplicateGroupsResponse> {
    let all = load_all_groups(db).await?;
    let totals = compute_totals(&all);
    let needle = search
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase);

    let groups = all
        .into_iter()
        .filter(|g| include_ignored || !g.ignored)
        .filter(|g| match &needle {
            None => true,
            Some(q) => {
                g.content_hash.to_lowercase().contains(q)
                    || g.members.iter().any(|m| {
                        m.filename.to_lowercase().contains(q) || m.path.to_lowercase().contains(q)
                    })
            }
        })
        .collect();

    Ok(DuplicateGroupsResponse { totals, groups })
}

pub async fn get_group_details(
    db: &Db,
    group_id: &str,
) -> AppResult<Option<DuplicateGroupSummary>> {
    let all = load_all_groups(db).await?;
    Ok(all.into_iter().find(|g| g.id == group_id))
}

pub async fn set_group_ignored(db: &Db, group_id: &str, ignored: bool) -> AppResult<bool> {
    let r = sqlx::query("UPDATE duplicate_groups SET ignored = ?2 WHERE id = ?1")
        .bind(group_id)
        .bind(if ignored { 1i64 } else { 0i64 })
        .execute(db)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn set_keeper(db: &Db, group_id: &str, file_record_id: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    let belongs: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM duplicate_members WHERE group_id = ?1 AND file_record_id = ?2",
    )
    .bind(group_id)
    .bind(file_record_id)
    .fetch_optional(&mut *tx)
    .await?;
    if belongs.is_none() {
        return Err(AppError::NotFound(format!(
            "duplicate member {file_record_id} in group {group_id}"
        )));
    }
    sqlx::query("UPDATE duplicate_members SET is_keeper = 0 WHERE group_id = ?1")
        .bind(group_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE duplicate_members SET is_keeper = 1 \
         WHERE group_id = ?1 AND file_record_id = ?2",
    )
    .bind(group_id)
    .bind(file_record_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Removes `file_record_id` from any duplicate group after it is trashed or moved out of scope,
/// deletes groups left with `< 2` copies, and promotes a new keeper if the removed member was keeper.
pub async fn remove_member_and_prune(
    db: &Db,
    group_id: &str,
    file_record_id: &str,
) -> AppResult<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM duplicate_members WHERE group_id = ?1 AND file_record_id = ?2")
        .bind(group_id)
        .bind(file_record_id)
        .execute(&mut *tx)
        .await?;

    let remaining: Vec<(String, i64)> = sqlx::query(
        "SELECT dm.file_record_id, dm.is_keeper \
         FROM duplicate_members dm \
         JOIN file_records fr ON fr.id = dm.file_record_id \
         WHERE dm.group_id = ?1 \
         ORDER BY COALESCE(fr.created_at_ns, fr.modified_at_ns) ASC, LENGTH(fr.path) ASC, fr.path ASC",
    )
    .bind(group_id)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|r| (r.get("file_record_id"), r.get("is_keeper")))
    .collect();

    if remaining.len() < 2 {
        sqlx::query("DELETE FROM duplicate_groups WHERE id = ?1")
            .bind(group_id)
            .execute(&mut *tx)
            .await?;
    } else if remaining.iter().all(|(_, k)| *k == 0) {
        let next_keeper = &remaining[0].0;
        sqlx::query(
            "UPDATE duplicate_members SET is_keeper = 1 \
             WHERE group_id = ?1 AND file_record_id = ?2",
        )
        .bind(group_id)
        .bind(next_keeper)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

async fn load_all_groups(db: &Db) -> AppResult<Vec<DuplicateGroupSummary>> {
    let group_rows = sqlx::query(
        "SELECT id, content_hash, file_size, ignored, created_at FROM duplicate_groups",
    )
    .fetch_all(db)
    .await?;

    let member_rows = sqlx::query(
        "SELECT dm.group_id, dm.file_record_id, dm.is_keeper, \
                fr.folder_id, fr.path, fr.filename, fr.extension, fr.size, \
                fr.created_at_ns, fr.modified_at_ns \
         FROM duplicate_members dm \
         JOIN file_records fr ON fr.id = dm.file_record_id \
         ORDER BY dm.is_keeper DESC, fr.filename ASC, fr.path ASC",
    )
    .fetch_all(db)
    .await?;

    let paths: Vec<String> = member_rows.iter().map(|r| r.get("path")).collect();
    let exists_flags = tokio::task::spawn_blocking(move || {
        paths
            .into_iter()
            .map(|p| Path::new(&p).symlink_metadata().is_ok())
            .collect::<Vec<bool>>()
    })
    .await
    .map_err(|e| crate::error::AppError::Internal(format!("stat task failed: {e}")))?;

    let mut members_by_group: HashMap<String, Vec<DuplicateMemberView>> = HashMap::new();
    for (r, exists) in member_rows.into_iter().zip(exists_flags) {
        let gid: String = r.get("group_id");
        let path_str: String = r.get("path");
        let size_i: i64 = r.get("size");
        let created_ns: Option<i64> = r.get("created_at_ns");
        let modified_ns: i64 = r.get("modified_at_ns");
        let is_keeper_i: i64 = r.get("is_keeper");
        members_by_group
            .entry(gid)
            .or_default()
            .push(DuplicateMemberView {
                file_record_id: r.get("file_record_id"),
                folder_id: r.get("folder_id"),
                path: path_str,
                filename: r.get("filename"),
                extension: r.get("extension"),
                size: u64::try_from(size_i).unwrap_or(0),
                created_ms: created_ns.unwrap_or(modified_ns) / 1_000_000,
                modified_ms: modified_ns / 1_000_000,
                is_keeper: is_keeper_i != 0,
                exists,
            });
    }

    let mut out = Vec::with_capacity(group_rows.len());
    for g in group_rows {
        let id: String = g.get("id");
        let members = members_by_group.remove(&id).unwrap_or_default();
        if members.len() < 2 {
            continue;
        }
        let file_size_i: i64 = g.get("file_size");
        let file_size = u64::try_from(file_size_i).unwrap_or(0);
        let ignored_i: i64 = g.get("ignored");
        let copy_count = u32::try_from(members.len()).unwrap_or(u32::MAX);
        let wasted_bytes = u64::from(copy_count.saturating_sub(1)).saturating_mul(file_size);
        let distinct_names: HashSet<&str> = members.iter().map(|m| m.filename.as_str()).collect();
        let all_same_name = distinct_names.len() <= 1;
        out.push(DuplicateGroupSummary {
            id,
            content_hash: g.get("content_hash"),
            file_size,
            ignored: ignored_i != 0,
            copy_count,
            wasted_bytes,
            all_same_name,
            same_content_different_names: !all_same_name,
            created_at: g.get("created_at"),
            members,
        });
    }

    out.sort_by(|a, b| {
        b.wasted_bytes
            .cmp(&a.wasted_bytes)
            .then_with(|| b.file_size.cmp(&a.file_size))
            .then_with(|| a.content_hash.cmp(&b.content_hash))
    });
    Ok(out)
}

fn compute_totals(groups: &[DuplicateGroupSummary]) -> DuplicateSummaryTotals {
    let mut totals = DuplicateSummaryTotals {
        total_groups: u32::try_from(groups.len()).unwrap_or(u32::MAX),
        active_groups: 0,
        ignored_groups: 0,
        total_duplicate_copies: 0,
        total_wasted_bytes: 0,
    };
    for g in groups {
        if g.ignored {
            totals.ignored_groups += 1;
        } else {
            totals.active_groups += 1;
            totals.total_duplicate_copies = totals
                .total_duplicate_copies
                .saturating_add(g.copy_count.saturating_sub(1));
            totals.total_wasted_bytes = totals.total_wasted_bytes.saturating_add(g.wasted_bytes);
        }
    }
    totals
}
