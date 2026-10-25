//! Printing lists: per-user model selections with quantities and plate settings. Users
//! can only see and change their own lists unless they are an admin.

use uuid::Uuid;

use crate::db::{Db, now};
use crate::dto::{PrintingListDetailDto, PrintingListItemDto, PrintingListPrinterInfoDto, PrintingListSummaryDto};
use crate::entities::{PrinterConfig, PrintingList, PrintingListItem};
use crate::error::{AppError, AppResult, not_found};
use crate::services::printers;

pub const DEFAULT_SPAWN_TYPE: &str = "grouped";
pub const DEFAULT_HULL_MODE: &str = "convex";

#[derive(Debug, Clone, Copy)]
pub struct Caller {
    pub user_id: Uuid,
    pub is_admin: bool,
}

impl Caller {
    fn can_access(&self, list: &PrintingList) -> bool {
        self.is_admin || list.owner_id == self.user_id
    }
}

pub fn normalize_spawn_type(spawn_type: Option<&str>) -> String {
    match spawn_type.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("random") => "random".into(),
        Some("largestfirstfillgaps") => "largestFirstFillGaps".into(),
        Some("bottomedge") | Some("center") => "bottomEdge".into(),
        Some("edges") => "edges".into(),
        _ => DEFAULT_SPAWN_TYPE.into(),
    }
}

pub fn normalize_hull_mode(hull_mode: Option<&str>) -> String {
    match hull_mode.map(|h| h.trim().to_ascii_lowercase()).as_deref() {
        Some("sansraft") => "sansRaft".into(),
        Some("concave") => "concave".into(),
        _ => DEFAULT_HULL_MODE.into(),
    }
}

async fn find_list(db: &Db, id: Uuid) -> sqlx::Result<Option<PrintingList>> {
    sqlx::query_as("SELECT * FROM printing_lists WHERE id = ?").bind(id).fetch_optional(db).await
}

async fn find_accessible(db: &Db, id: Uuid, caller: Caller) -> AppResult<PrintingList> {
    find_list(db, id).await?.filter(|l| caller.can_access(l)).ok_or_else(not_found)
}

async fn owner_username(db: &Db, owner_id: Uuid) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar("SELECT username FROM users WHERE id = ?").bind(owner_id).fetch_optional(db).await
}

async fn item_count(db: &Db, list_id: Uuid) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COALESCE(SUM(quantity), 0) FROM printing_list_items WHERE printing_list_id = ?")
        .bind(list_id)
        .fetch_one(db)
        .await
}

async fn summary(db: &Db, list: &PrintingList) -> sqlx::Result<PrintingListSummaryDto> {
    Ok(PrintingListSummaryDto {
        id: list.id,
        name: list.name.clone(),
        is_active: list.is_active,
        is_default: list.is_default,
        spawn_type: list.spawn_type.clone(),
        hull_mode: list.hull_mode.clone(),
        created_at: list.created_at,
        owner_username: owner_username(db, list.owner_id).await?,
        item_count: item_count(db, list.id).await?,
    })
}

async fn detail(db: &Db, list: &PrintingList) -> sqlx::Result<PrintingListDetailDto> {
    let items: Vec<PrintingListItem> = sqlx::query_as("SELECT * FROM printing_list_items WHERE printing_list_id = ?")
        .bind(list.id)
        .fetch_all(db)
        .await?;
    let printer: Option<PrinterConfig> = match list.printer_config_id {
        Some(pid) => sqlx::query_as("SELECT * FROM printer_configs WHERE id = ?").bind(pid).fetch_optional(db).await?,
        None => None,
    };
    Ok(PrintingListDetailDto {
        id: list.id,
        name: list.name.clone(),
        is_active: list.is_active,
        is_default: list.is_default,
        spawn_type: normalize_spawn_type(Some(&list.spawn_type)),
        hull_mode: normalize_hull_mode(Some(&list.hull_mode)),
        created_at: list.created_at,
        owner_username: owner_username(db, list.owner_id).await?,
        items: items
            .iter()
            .map(|i| PrintingListItemDto { id: i.id, model_id: i.model_id, quantity: i.quantity })
            .collect(),
        printer: printer.map(|p| PrintingListPrinterInfoDto {
            id: p.id,
            name: p.name,
            bed_width_mm: p.bed_width_mm,
            bed_depth_mm: p.bed_depth_mm,
        }),
    })
}

async fn insert_list(db: &Db, list: &PrintingList) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO printing_lists (id, name, owner_id, is_active, is_default, spawn_type, hull_mode, printer_config_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(list.id)
    .bind(&list.name)
    .bind(list.owner_id)
    .bind(list.is_active)
    .bind(list.is_default)
    .bind(&list.spawn_type)
    .bind(&list.hull_mode)
    .bind(list.printer_config_id)
    .bind(list.created_at)
    .execute(db)
    .await?;
    Ok(())
}

/// Ensures the user has a default list (made active when created).
pub async fn ensure_default_list(db: &Db, user_id: Uuid) -> sqlx::Result<()> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM printing_lists WHERE owner_id = ? AND is_default = 1)")
            .bind(user_id)
            .fetch_one(db)
            .await?;
    if exists {
        return Ok(());
    }
    sqlx::query("UPDATE printing_lists SET is_active = 0 WHERE owner_id = ?").bind(user_id).execute(db).await?;
    insert_list(
        db,
        &PrintingList {
            id: Uuid::new_v4(),
            name: "Default".into(),
            owner_id: user_id,
            is_active: true,
            is_default: true,
            spawn_type: DEFAULT_SPAWN_TYPE.into(),
            hull_mode: DEFAULT_HULL_MODE.into(),
            printer_config_id: printers::default_printer_id(db).await?,
            created_at: now(),
        },
    )
    .await
}

pub async fn get_lists(db: &Db, caller: Caller) -> AppResult<Vec<PrintingListSummaryDto>> {
    let lists: Vec<PrintingList> = if caller.is_admin {
        sqlx::query_as("SELECT * FROM printing_lists ORDER BY created_at DESC").fetch_all(db).await?
    } else {
        sqlx::query_as("SELECT * FROM printing_lists WHERE owner_id = ? ORDER BY created_at DESC")
            .bind(caller.user_id)
            .fetch_all(db)
            .await?
    };
    let mut out = Vec::with_capacity(lists.len());
    for list in &lists {
        out.push(summary(db, list).await?);
    }
    Ok(out)
}

pub async fn get_active_list(db: &Db, user_id: Uuid) -> AppResult<Option<PrintingListDetailDto>> {
    let list: Option<PrintingList> =
        sqlx::query_as("SELECT * FROM printing_lists WHERE owner_id = ? AND is_active = 1 LIMIT 1")
            .bind(user_id)
            .fetch_optional(db)
            .await?;
    Ok(match list {
        Some(l) => Some(detail(db, &l).await?),
        None => None,
    })
}

pub async fn get_list(db: &Db, id: Uuid, caller: Caller) -> AppResult<PrintingListDetailDto> {
    let list = find_accessible(db, id, caller).await?;
    Ok(detail(db, &list).await?)
}

pub async fn create_list(db: &Db, user_id: Uuid, name: &str) -> AppResult<PrintingListSummaryDto> {
    let has_any: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM printing_lists WHERE owner_id = ?)")
        .bind(user_id)
        .fetch_one(db)
        .await?;
    let list = PrintingList {
        id: Uuid::new_v4(),
        name: name.to_string(),
        owner_id: user_id,
        is_active: !has_any,
        is_default: false,
        spawn_type: DEFAULT_SPAWN_TYPE.into(),
        hull_mode: DEFAULT_HULL_MODE.into(),
        printer_config_id: printers::default_printer_id(db).await?,
        created_at: now(),
    };
    insert_list(db, &list).await?;
    Ok(summary(db, &list).await?)
}

pub async fn rename_list(db: &Db, id: Uuid, caller: Caller, name: &str) -> AppResult<PrintingListSummaryDto> {
    let mut list = find_accessible(db, id, caller).await?;
    if list.is_default {
        return Err(AppError::Conflict("The default list cannot be renamed.".into()));
    }
    sqlx::query("UPDATE printing_lists SET name = ? WHERE id = ?").bind(name).bind(id).execute(db).await?;
    list.name = name.to_string();
    Ok(summary(db, &list).await?)
}

pub async fn update_settings(
    db: &Db,
    id: Uuid,
    caller: Caller,
    spawn_type: &str,
    hull_mode: &str,
) -> AppResult<PrintingListDetailDto> {
    let mut list = find_accessible(db, id, caller).await?;
    list.spawn_type = normalize_spawn_type(Some(spawn_type));
    list.hull_mode = normalize_hull_mode(Some(hull_mode));
    sqlx::query("UPDATE printing_lists SET spawn_type = ?, hull_mode = ? WHERE id = ?")
        .bind(&list.spawn_type)
        .bind(&list.hull_mode)
        .bind(id)
        .execute(db)
        .await?;
    Ok(detail(db, &list).await?)
}

pub async fn update_printer(
    db: &Db,
    id: Uuid,
    caller: Caller,
    printer_id: Option<Uuid>,
) -> AppResult<PrintingListDetailDto> {
    let mut list = find_accessible(db, id, caller).await?;
    if let Some(pid) = printer_id {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM printer_configs WHERE id = ?)")
            .bind(pid)
            .fetch_one(db)
            .await?;
        if !exists {
            return Err(not_found());
        }
    }
    sqlx::query("UPDATE printing_lists SET printer_config_id = ? WHERE id = ?")
        .bind(printer_id)
        .bind(id)
        .execute(db)
        .await?;
    list.printer_config_id = printer_id;
    Ok(detail(db, &list).await?)
}

pub async fn delete_list(db: &Db, id: Uuid, caller: Caller) -> AppResult<()> {
    let list = find_accessible(db, id, caller).await?;
    if list.is_default {
        return Err(AppError::Conflict("The default list cannot be deleted.".into()));
    }
    sqlx::query("DELETE FROM printing_lists WHERE id = ?").bind(id).execute(db).await?;
    if list.is_active {
        sqlx::query(
            "UPDATE printing_lists SET is_active = 1 WHERE id = (SELECT id FROM printing_lists WHERE owner_id = ? ORDER BY created_at LIMIT 1)",
        )
        .bind(list.owner_id)
        .execute(db)
        .await?;
    }
    Ok(())
}

/// Resolves "active" (the caller's active list) or a list id.
pub async fn resolve_list_id(db: &Db, id: &str, user_id: Uuid) -> AppResult<Option<Uuid>> {
    if id.eq_ignore_ascii_case("active") {
        return Ok(sqlx::query_scalar("SELECT id FROM printing_lists WHERE owner_id = ? AND is_active = 1 LIMIT 1")
            .bind(user_id)
            .fetch_optional(db)
            .await?);
    }
    Ok(Uuid::parse_str(id).ok())
}

pub async fn activate_list(db: &Db, id: Uuid, caller: Caller) -> AppResult<()> {
    let list = find_accessible(db, id, caller).await?;
    let mut tx = db.begin().await?;
    sqlx::query("UPDATE printing_lists SET is_active = (id = ?) WHERE owner_id = ?")
        .bind(id)
        .bind(list.owner_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Sets an item's quantity; zero or less removes it.
pub async fn upsert_item(
    db: &Db,
    list_id: Uuid,
    caller: Caller,
    model_id: Uuid,
    quantity: i32,
) -> AppResult<PrintingListDetailDto> {
    let list = find_accessible(db, list_id, caller).await?;
    if quantity <= 0 {
        sqlx::query("DELETE FROM printing_list_items WHERE printing_list_id = ? AND model_id = ?")
            .bind(list_id)
            .bind(model_id)
            .execute(db)
            .await?;
    } else {
        sqlx::query(
            "INSERT INTO printing_list_items (id, printing_list_id, model_id, quantity) VALUES (?, ?, ?, ?)
             ON CONFLICT(printing_list_id, model_id) DO UPDATE SET quantity = excluded.quantity",
        )
        .bind(Uuid::new_v4())
        .bind(list_id)
        .bind(model_id)
        .bind(quantity)
        .execute(db)
        .await?;
    }
    Ok(detail(db, &list).await?)
}

pub async fn clear_items(db: &Db, list_id: Uuid, caller: Caller) -> AppResult<PrintingListDetailDto> {
    let list = find_accessible(db, list_id, caller).await?;
    sqlx::query("DELETE FROM printing_list_items WHERE printing_list_id = ?").bind(list_id).execute(db).await?;
    Ok(detail(db, &list).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDb;

    async fn setup() -> (TestDb, Uuid, Caller) {
        let t = TestDb::new().await;
        let user = t.add_user("jamie", false).await;
        (t, user, Caller { user_id: user, is_admin: false })
    }

    async fn add_printer(db: &Db, name: &str, default: bool) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO printer_configs (id, name, bed_width_mm, bed_depth_mm, is_built_in, is_default) VALUES (?, ?, 218, 123, 0, ?)")
            .bind(id)
            .bind(name)
            .bind(default)
            .execute(db)
            .await
            .unwrap();
        id
    }

    #[tokio::test]
    async fn create_list_assigns_default_printer() {
        let (t, user, caller) = setup().await;
        sqlx::query("UPDATE printer_configs SET is_default = 0").execute(&t.db).await.unwrap();
        let printer = add_printer(&t.db, "Uniformation GK2", true).await;

        let created = create_list(&t.db, user, "Test").await.unwrap();
        let detail = get_list(&t.db, created.id, caller).await.unwrap();
        assert_eq!(detail.printer.unwrap().id, printer);
    }

    #[tokio::test]
    async fn update_printer_changes_selected_printer() {
        let (t, user, caller) = setup().await;
        let second = add_printer(&t.db, "Saturn 4 Ultra", false).await;
        let created = create_list(&t.db, user, "Test").await.unwrap();
        let updated = update_printer(&t.db, created.id, caller, Some(second)).await.unwrap();
        assert_eq!(updated.printer.unwrap().id, second);
        assert!(matches!(
            update_printer(&t.db, created.id, caller, Some(Uuid::new_v4())).await,
            Err(AppError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn default_list_cannot_be_renamed_or_deleted_and_first_list_is_active() {
        let (t, user, caller) = setup().await;
        ensure_default_list(&t.db, user).await.unwrap();
        ensure_default_list(&t.db, user).await.unwrap();
        let lists = get_lists(&t.db, caller).await.unwrap();
        assert_eq!(lists.len(), 1);
        let default = &lists[0];
        assert!(default.is_active && default.is_default);
        assert!(matches!(rename_list(&t.db, default.id, caller, "x").await, Err(AppError::Conflict(_))));
        assert!(matches!(delete_list(&t.db, default.id, caller).await, Err(AppError::Conflict(_))));
    }

    #[tokio::test]
    async fn items_upsert_remove_and_count() {
        let (t, user, caller) = setup().await;
        let list = create_list(&t.db, user, "Test").await.unwrap();
        let model = Uuid::new_v4();
        upsert_item(&t.db, list.id, caller, model, 2).await.unwrap();
        let detail = upsert_item(&t.db, list.id, caller, model, 3).await.unwrap();
        assert_eq!(detail.items.len(), 1);
        assert_eq!(detail.items[0].quantity, 3);
        assert_eq!(get_lists(&t.db, caller).await.unwrap()[0].item_count, 3);
        let detail = upsert_item(&t.db, list.id, caller, model, 0).await.unwrap();
        assert!(detail.items.is_empty());
    }

    #[tokio::test]
    async fn other_users_cannot_access_lists_but_admins_can() {
        let (t, user, _) = setup().await;
        let list = create_list(&t.db, user, "Private").await.unwrap();
        let other = Caller { user_id: t.add_user("other", false).await, is_admin: false };
        assert!(matches!(get_list(&t.db, list.id, other).await, Err(AppError::NotFound(_))));
        assert!(matches!(upsert_item(&t.db, list.id, other, Uuid::new_v4(), 1).await, Err(AppError::NotFound(_))));
        let admin = Caller { user_id: t.add_user("root", true).await, is_admin: true };
        assert!(get_list(&t.db, list.id, admin).await.is_ok());
    }

    #[tokio::test]
    async fn deleting_active_list_activates_oldest_remaining() {
        let (t, user, caller) = setup().await;
        let first = create_list(&t.db, user, "First").await.unwrap();
        let second = create_list(&t.db, user, "Second").await.unwrap();
        activate_list(&t.db, second.id, caller).await.unwrap();
        delete_list(&t.db, second.id, caller).await.unwrap();
        let active = get_active_list(&t.db, user).await.unwrap().unwrap();
        assert_eq!(active.id, first.id);
        assert_eq!(resolve_list_id(&t.db, "active", user).await.unwrap(), Some(first.id));
    }

    #[test]
    fn settings_normalisation() {
        assert_eq!(normalize_spawn_type(Some("RANDOM")), "random");
        assert_eq!(normalize_spawn_type(Some("largestFirstFillGaps")), "largestFirstFillGaps");
        assert_eq!(normalize_spawn_type(Some("bottomEdge")), "bottomEdge");
        assert_eq!(normalize_spawn_type(Some("Center")), "bottomEdge");
        assert_eq!(normalize_spawn_type(Some("EDGES")), "edges");
        assert_eq!(normalize_spawn_type(None), "grouped");
        assert_eq!(normalize_hull_mode(Some("sansraft")), "sansRaft");
        assert_eq!(normalize_hull_mode(Some("Concave")), "concave");
        assert_eq!(normalize_hull_mode(Some("other")), "convex");
    }
}
