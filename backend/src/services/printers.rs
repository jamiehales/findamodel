//! Printer configurations (bed sizes for plate layout).

use uuid::Uuid;

use crate::db::Db;
use crate::dto::{PrinterConfigDto, PrinterConfigRequest};
use crate::entities::PrinterConfig;
use crate::error::{AppError, AppResult, bad_request, not_found};

fn to_dto(p: &PrinterConfig) -> PrinterConfigDto {
    PrinterConfigDto {
        id: p.id,
        name: p.name.clone(),
        bed_width_mm: p.bed_width_mm,
        bed_depth_mm: p.bed_depth_mm,
        is_built_in: p.is_built_in,
        is_default: p.is_default,
    }
}

fn validate(req: &PrinterConfigRequest) -> AppResult<(String, f32, f32)> {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(bad_request("Name is required."));
    }
    if req.bed_width_mm <= 0.0 || req.bed_depth_mm <= 0.0 {
        return Err(bad_request("Bed dimensions must be positive."));
    }
    Ok((name, req.bed_width_mm, req.bed_depth_mm))
}

pub async fn get_all(db: &Db) -> AppResult<Vec<PrinterConfigDto>> {
    let rows: Vec<PrinterConfig> =
        sqlx::query_as("SELECT * FROM printer_configs ORDER BY is_built_in DESC, name").fetch_all(db).await?;
    Ok(rows.iter().map(to_dto).collect())
}

async fn find(db: &Db, id: Uuid) -> AppResult<Option<PrinterConfig>> {
    Ok(sqlx::query_as("SELECT * FROM printer_configs WHERE id = ?").bind(id).fetch_optional(db).await?)
}

pub async fn create(db: &Db, req: &PrinterConfigRequest) -> AppResult<PrinterConfigDto> {
    let (name, width, depth) = validate(req)?;
    let printer = PrinterConfig {
        id: Uuid::new_v4(),
        name,
        bed_width_mm: width,
        bed_depth_mm: depth,
        is_built_in: false,
        is_default: false,
    };
    sqlx::query("INSERT INTO printer_configs (id, name, bed_width_mm, bed_depth_mm, is_built_in, is_default) VALUES (?, ?, ?, ?, 0, 0)")
        .bind(printer.id)
        .bind(&printer.name)
        .bind(printer.bed_width_mm)
        .bind(printer.bed_depth_mm)
        .execute(db)
        .await?;
    Ok(to_dto(&printer))
}

pub async fn update(db: &Db, id: Uuid, req: &PrinterConfigRequest) -> AppResult<PrinterConfigDto> {
    let (name, width, depth) = validate(req)?;
    let mut printer = find(db, id).await?.ok_or_else(not_found)?;
    if printer.is_built_in {
        return Err(bad_request("Built-in printers cannot be modified."));
    }
    sqlx::query("UPDATE printer_configs SET name = ?, bed_width_mm = ?, bed_depth_mm = ? WHERE id = ?")
        .bind(&name)
        .bind(width)
        .bind(depth)
        .bind(id)
        .execute(db)
        .await?;
    printer.name = name;
    printer.bed_width_mm = width;
    printer.bed_depth_mm = depth;
    Ok(to_dto(&printer))
}

pub async fn delete(db: &Db, id: Uuid) -> AppResult<()> {
    let printer = find(db, id).await?.ok_or_else(not_found)?;
    if printer.is_built_in {
        return Err(AppError::Conflict("Built-in printers cannot be deleted.".into()));
    }
    if printer.is_default {
        return Err(AppError::Conflict(
            "Cannot delete the default printer. Set another printer as default first.".into(),
        ));
    }
    sqlx::query("DELETE FROM printer_configs WHERE id = ?").bind(id).execute(db).await?;
    Ok(())
}

pub async fn set_default(db: &Db, id: Uuid) -> AppResult<()> {
    find(db, id).await?.ok_or_else(not_found)?;
    sqlx::query("UPDATE printer_configs SET is_default = (id = ?)").bind(id).execute(db).await?;
    Ok(())
}

pub async fn default_printer_id(db: &Db) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar("SELECT id FROM printer_configs WHERE is_default = 1 LIMIT 1").fetch_optional(db).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDb;

    async fn clear(db: &Db) {
        sqlx::query("DELETE FROM printer_configs").execute(db).await.unwrap();
    }

    async fn add(db: &Db, name: &str, built_in: bool, default: bool) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO printer_configs (id, name, bed_width_mm, bed_depth_mm, is_built_in, is_default) VALUES (?, ?, 228, 128, ?, ?)")
            .bind(id)
            .bind(name)
            .bind(built_in)
            .bind(default)
            .execute(db)
            .await
            .unwrap();
        id
    }

    #[tokio::test]
    async fn seeded_printers_include_default_gk3() {
        let t = TestDb::new().await;
        let printers = get_all(&t.db).await.unwrap();
        assert_eq!(printers.len(), 2);
        let default: Vec<_> = printers.iter().filter(|p| p.is_default).collect();
        assert_eq!(default.len(), 1);
        assert_eq!(default[0].name, "Uniformation GK3");
    }

    #[tokio::test]
    async fn create_makes_custom_printer() {
        let t = TestDb::new().await;
        let dto = create(
            &t.db,
            &PrinterConfigRequest { name: "Saturn 4 Ultra".into(), bed_width_mm: 218.0, bed_depth_mm: 123.0 },
        )
        .await
        .unwrap();
        assert!(!dto.is_built_in && !dto.is_default);
        assert_eq!(dto.name, "Saturn 4 Ultra");
        let err = create(&t.db, &PrinterConfigRequest { name: " ".into(), bed_width_mm: 1.0, bed_depth_mm: 1.0 }).await;
        assert!(matches!(err, Err(AppError::BadRequest(_))));
    }

    #[tokio::test]
    async fn delete_rejects_built_in_printer() {
        let t = TestDb::new().await;
        clear(&t.db).await;
        let id = add(&t.db, "Uniformation GK2", true, true).await;
        match delete(&t.db, id).await {
            Err(AppError::Conflict(m)) => assert_eq!(m, "Built-in printers cannot be deleted."),
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(delete(&t.db, Uuid::new_v4()).await, Err(AppError::NotFound(_))));
    }

    #[tokio::test]
    async fn set_default_switches_default_printer() {
        let t = TestDb::new().await;
        clear(&t.db).await;
        let old = add(&t.db, "Uniformation GK2", true, true).await;
        let new = add(&t.db, "Saturn 4 Ultra", false, false).await;
        set_default(&t.db, new).await.unwrap();
        assert_eq!(default_printer_id(&t.db).await.unwrap(), Some(new));
        let old_row = find(&t.db, old).await.unwrap().unwrap();
        assert!(!old_row.is_default);
    }
}
