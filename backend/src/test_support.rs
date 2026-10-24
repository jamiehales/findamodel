//! Helpers for tests that need a real database and models directory.

use std::path::{Path, PathBuf};

use chrono::Utc;
use uuid::Uuid;

use crate::db::{self, Db};
use crate::entities::ModelRow;
use crate::services::store;

/// A migrated SQLite database in a temporary directory.
pub struct TestDb {
    pub db: Db,
    pub dir: tempfile::TempDir,
}

impl TestDb {
    pub async fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = db::open(&dir.path().join("test.db")).await.expect("open test db");
        TestDb { db, dir }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub async fn insert_model(&self, model: &ModelRow) {
        let mut conn = self.db.acquire().await.unwrap();
        store::save_model(&mut conn, model).await.unwrap();
    }

    pub async fn add_user(&self, name: &str, is_admin: bool) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO users (id, username, is_admin) VALUES (?, ?, ?)")
            .bind(id)
            .bind(name)
            .bind(is_admin)
            .execute(&self.db)
            .await
            .unwrap();
        id
    }
}

/// A model row with the required fields filled in.
pub fn model(file_name: &str, directory: &str) -> ModelRow {
    let file_type = Path::new(file_name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    ModelRow {
        id: Uuid::new_v4(),
        checksum: format!("checksum-{file_name}"),
        file_name: file_name.to_string(),
        directory: directory.to_string(),
        file_type,
        file_size: 1,
        file_modified_at: Utc::now(),
        cached_at: Utc::now(),
        ..Default::default()
    }
}

pub const SIMPLE_ASCII_STL: &str = "solid triangle
  facet normal 0 0 1
    outer loop
      vertex 0 0 0
      vertex 10 0 0
      vertex 0 10 0
    endloop
  endfacet
endsolid triangle
";

pub fn write_file(dir: &Path, relative: &str, contents: impl AsRef<[u8]>) -> PathBuf {
    let path = dir.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, contents).unwrap();
    path
}
