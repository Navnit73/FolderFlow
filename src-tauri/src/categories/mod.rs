//! File categories: built-ins (FR-030) and user-defined custom categories (FR-034/035).

mod model;

pub use model::*;

use crate::database::{repo_categories, Db};
use crate::error::{AppError, AppResult};

/// Normalises and validates a custom category: trims the name, lower-cases
/// and de-duplicates extensions (leading dots stripped), requires a theme
/// color token.
pub fn normalize_input(mut input: CategoryInput) -> AppResult<CategoryInput> {
    input.name = input.name.trim().to_owned();
    if input.name.is_empty() || input.name.chars().count() > 60 {
        return Err(AppError::validation(
            "name",
            "Name must be 1–60 characters.",
        ));
    }
    let mut exts: Vec<String> = Vec::new();
    for raw in &input.extensions {
        let e = raw.trim().trim_start_matches('.').to_lowercase();
        if e.is_empty()
            || e.len() > 20
            || !e
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(AppError::validation(
                "extensions",
                format!("\"{raw}\" is not a valid extension."),
            ));
        }
        if !exts.contains(&e) {
            exts.push(e);
        }
    }
    if exts.is_empty() {
        return Err(AppError::validation(
            "extensions",
            "Add at least one extension.",
        ));
    }
    input.extensions = exts;
    if !COLOR_TOKENS.contains(&input.color_token.as_str()) {
        return Err(AppError::validation(
            "colorToken",
            "Choose one of the category colors.",
        ));
    }
    input.icon = input.icon.trim().to_owned();
    if input.icon.is_empty() || input.icon.len() > 40 {
        return Err(AppError::validation("icon", "Choose an icon."));
    }
    if !(0..=1000).contains(&input.priority) {
        return Err(AppError::validation(
            "priority",
            "Priority must be between 0 and 1000.",
        ));
    }
    input.description = input
        .description
        .map(|d| d.trim().to_owned())
        .filter(|d| !d.is_empty());
    Ok(input)
}

#[derive(Clone)]
pub struct CategoryService {
    db: Db,
}

impl CategoryService {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Inserts missing built-ins and refreshes their name/icon/extensions,
    /// preserving the user's `enabled`, `priority` and `destination_path`.
    pub async fn ensure_builtins(&self) -> AppResult<()> {
        for c in builtin_categories() {
            repo_categories::upsert_builtin(&self.db, &c).await?;
        }
        Ok(())
    }

    pub async fn list(&self) -> AppResult<Vec<Category>> {
        repo_categories::list(&self.db).await
    }

    async fn ensure_unique_name(&self, name: &str, except_id: Option<&str>) -> AppResult<()> {
        let clash = self
            .list()
            .await?
            .into_iter()
            .any(|c| c.name.eq_ignore_ascii_case(name) && Some(c.id.as_str()) != except_id);
        if clash {
            return Err(AppError::validation(
                "name",
                "A category with this name already exists.",
            ));
        }
        Ok(())
    }

    pub async fn create(&self, input: CategoryInput) -> AppResult<Category> {
        let input = normalize_input(input)?;
        self.ensure_unique_name(&input.name, None).await?;
        let c = Category {
            id: uuid::Uuid::now_v7().to_string(),
            name: input.name,
            description: input.description,
            icon: input.icon,
            color_token: input.color_token,
            extensions: input.extensions,
            destination_path: input.destination_path,
            priority: input.priority,
            builtin: false,
            enabled: input.enabled,
        };
        repo_categories::insert(&self.db, &c).await?;
        Ok(c)
    }

    pub async fn update(&self, id: &str, input: CategoryInput) -> AppResult<Category> {
        let existing = repo_categories::get(&self.db, id)
            .await?
            .ok_or_else(|| AppError::NotFound("Category".into()))?;
        if existing.builtin {
            return Err(AppError::validation(
                "id",
                "Built-in categories can only be enabled or disabled. Create a custom category instead.",
            ));
        }
        let input = normalize_input(input)?;
        self.ensure_unique_name(&input.name, Some(id)).await?;
        let c = Category {
            id: existing.id,
            name: input.name,
            description: input.description,
            icon: input.icon,
            color_token: input.color_token,
            extensions: input.extensions,
            destination_path: input.destination_path,
            priority: input.priority,
            builtin: false,
            enabled: input.enabled,
        };
        repo_categories::update(&self.db, &c).await?;
        Ok(c)
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> AppResult<Category> {
        if id == OTHER && !enabled {
            return Err(AppError::validation(
                "id",
                "The Other category can't be disabled.",
            ));
        }
        if !repo_categories::set_enabled(&self.db, id, enabled).await? {
            return Err(AppError::NotFound("Category".into()));
        }
        repo_categories::get(&self.db, id)
            .await?
            .ok_or_else(|| AppError::NotFound("Category".into()))
    }

    pub async fn delete(&self, id: &str) -> AppResult<()> {
        match repo_categories::get(&self.db, id).await? {
            None => Err(AppError::NotFound("Category".into())),
            Some(c) if c.builtin => Err(AppError::validation(
                "id",
                "Built-in categories can't be deleted.",
            )),
            Some(_) => {
                repo_categories::delete_custom(&self.db, id).await?;
                Ok(())
            }
        }
    }

    /// Updates category priorities following the given ordered list of IDs (`FR-170`).
    /// Unknown IDs are rejected; any existing categories omitted from `ordered_ids` keep relative order afterward.
    pub async fn reorder(&self, ordered_ids: Vec<String>) -> AppResult<Vec<Category>> {
        let existing = self.list().await?;
        let mut seen = std::collections::HashSet::with_capacity(ordered_ids.len());
        let mut final_order = Vec::with_capacity(existing.len());

        for id in ordered_ids {
            if !existing.iter().any(|c| c.id == id) {
                return Err(AppError::NotFound(format!("Category {id}")));
            }
            if seen.insert(id.clone()) {
                final_order.push(id);
            }
        }
        for c in existing {
            if seen.insert(c.id.clone()) {
                final_order.push(c.id);
            }
        }

        repo_categories::reorder(&self.db, &final_order).await?;
        self.list().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;

    fn input(name: &str, exts: &[&str]) -> CategoryInput {
        CategoryInput {
            name: name.into(),
            description: None,
            icon: "receipt".into(),
            color_token: "cat-documents".into(),
            extensions: exts.iter().map(|e| (*e).to_owned()).collect(),
            destination_path: None,
            priority: 50,
            enabled: true,
        }
    }

    async fn service() -> CategoryService {
        let svc = CategoryService::new(database::open_in_memory().await.expect("db"));
        svc.ensure_builtins().await.expect("seed");
        svc
    }

    #[tokio::test]
    async fn r_cat_02_builtins_seeded_idempotently_and_user_changes_preserved() {
        let svc = service().await;
        let all = svc.list().await.expect("list");
        assert_eq!(all.iter().filter(|c| c.builtin).count(), 13);
        svc.set_enabled("images", false).await.expect("disable");
        svc.ensure_builtins().await.expect("re-seed");
        let images = svc
            .list()
            .await
            .expect("list")
            .into_iter()
            .find(|c| c.id == "images")
            .expect("images");
        assert!(
            !images.enabled,
            "re-seeding must not re-enable a category the user disabled"
        );
        assert_eq!(svc.list().await.expect("list").len(), 13);
    }

    #[tokio::test]
    async fn r_cat_02b_custom_crud_and_builtins_cannot_be_deleted() {
        let svc = service().await;
        let created = svc
            .create(input("Invoices", &[".PDF", "pdf", " xml "]))
            .await
            .expect("create");
        assert!(!created.builtin);
        assert_eq!(
            created.extensions,
            vec!["pdf", "xml"],
            "normalised + deduplicated"
        );
        let updated = svc
            .update(&created.id, input("Bills", &["pdf"]))
            .await
            .expect("update");
        assert_eq!(updated.name, "Bills");
        assert!(matches!(
            svc.delete("documents").await,
            Err(AppError::Validation { .. })
        ));
        svc.delete(&created.id).await.expect("delete custom");
        assert!(matches!(
            svc.delete(&created.id).await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            svc.update("documents", input("Docs", &["pdf"])).await,
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn r_cat_03_validation() {
        assert!(normalize_input(input("", &["pdf"])).is_err(), "empty name");
        assert!(normalize_input(input("X", &[])).is_err(), "no extensions");
        assert!(
            normalize_input(input("X", &["p/df"])).is_err(),
            "bad extension"
        );
        let mut bad_color = input("X", &["pdf"]);
        bad_color.color_token = "#ff0000".into();
        assert!(
            normalize_input(bad_color).is_err(),
            "colors must be theme tokens"
        );
        let ok = normalize_input(input("  Receipts ", &["PDF", ".Jpg"])).expect("valid");
        assert_eq!(ok.name, "Receipts");
        assert_eq!(ok.extensions, vec!["pdf", "jpg"]);
    }

    #[tokio::test]
    async fn r_cat_03b_names_are_unique_case_insensitively() {
        let svc = service().await;
        assert!(matches!(
            svc.create(input("documents", &["foo"])).await,
            Err(AppError::Validation { .. })
        ));
    }

    #[tokio::test]
    async fn r_cat_04_reorder_persists_priority_order() {
        let svc = service().await;
        let custom = svc
            .create(input("Receipts", &["rcpt"]))
            .await
            .expect("create");
        let listed = svc.list().await.expect("list");
        let original_first = listed[0].id.clone();

        let reversed_two = vec!["images".to_owned(), custom.id.clone(), original_first];
        let reordered = svc.reorder(reversed_two).await.expect("reorder");
        assert_eq!(reordered[0].id, "images");
        assert_eq!(reordered[0].priority, 10);
        assert_eq!(reordered[1].id, custom.id);
        assert_eq!(reordered[1].priority, 20);

        assert!(matches!(
            svc.reorder(vec!["does-not-exist".into()]).await,
            Err(AppError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn edge_06_custom_category_and_priority_drive_live_classifier_resolution() {
        use crate::engine::classifier::Classifier;

        let svc = service().await;
        // Override .pdf at higher priority (smaller number = higher priority than built-in documents priority 10)
        let legal = svc
            .create(CategoryInput {
                name: "Legal Briefs".into(),
                description: None,
                icon: "scale".into(),
                color_token: "cat-documents".into(),
                extensions: vec!["pdf".into(), "parquet".into()],
                destination_path: None,
                priority: 5,
                enabled: true,
            })
            .await
            .expect("create legal");

        let cats = svc.list().await.expect("list");
        let cls = Classifier::new(cats);
        assert_eq!(cls.classify("brief.pdf", None, false).category, legal.id);
        assert_eq!(
            cls.classify("table.parquet", None, false).category,
            legal.id
        );

        // Reordering `"documents"` ahead of `"Legal Briefs"` restores `"documents"` precedence for `.pdf`
        let reordered = svc
            .reorder(vec!["documents".to_owned(), legal.id.clone()])
            .await
            .expect("reorder");
        let cls2 = Classifier::new(reordered);
        assert_eq!(
            cls2.classify("brief.pdf", None, false).category,
            "documents"
        );
        assert_eq!(
            cls2.classify("table.parquet", None, false).category,
            legal.id
        );
    }
}
