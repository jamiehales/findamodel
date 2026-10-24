//! Users. A single admin user is seeded and every request authenticates as that user.

use uuid::Uuid;

use crate::db::Db;
use crate::entities::User;

pub async fn seed_admin_user(db: &Db) -> sqlx::Result<()> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(db).await?;
    if count == 0 {
        sqlx::query("INSERT OR IGNORE INTO users (id, username, is_admin) VALUES (?, 'admin', 1)")
            .bind(Uuid::new_v4())
            .execute(db)
            .await?;
    }
    Ok(())
}

pub async fn get_user_by_id(db: &Db, id: Uuid) -> sqlx::Result<Option<User>> {
    sqlx::query_as("SELECT * FROM users WHERE id = ?").bind(id).fetch_optional(db).await
}

pub async fn get_admin_user(db: &Db) -> sqlx::Result<Option<User>> {
    sqlx::query_as("SELECT * FROM users WHERE is_admin = 1 ORDER BY username LIMIT 1").fetch_optional(db).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDb;

    async fn add_user(db: &Db, id: Uuid, name: &str, admin: bool) {
        sqlx::query("INSERT INTO users (id, username, is_admin) VALUES (?, ?, ?)")
            .bind(id)
            .bind(name)
            .bind(admin)
            .execute(db)
            .await
            .unwrap();
    }

    async fn count(db: &Db) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(db).await.unwrap()
    }

    #[tokio::test]
    async fn seed_creates_admin_when_empty() {
        let t = TestDb::new().await;
        seed_admin_user(&t.db).await.unwrap();
        let user: User = sqlx::query_as("SELECT * FROM users").fetch_one(&t.db).await.unwrap();
        assert_eq!(user.username, "admin");
        assert!(user.is_admin);
    }

    #[tokio::test]
    async fn seed_is_idempotent() {
        let t = TestDb::new().await;
        add_user(&t.db, Uuid::new_v4(), "admin", true).await;
        seed_admin_user(&t.db).await.unwrap();
        assert_eq!(count(&t.db).await, 1);

        let empty = TestDb::new().await;
        seed_admin_user(&empty.db).await.unwrap();
        seed_admin_user(&empty.db).await.unwrap();
        assert_eq!(count(&empty.db).await, 1);
    }

    #[tokio::test]
    async fn get_user_by_id_finds_existing_users_only() {
        let t = TestDb::new().await;
        assert!(get_user_by_id(&t.db, Uuid::new_v4()).await.unwrap().is_none());
        let id = Uuid::new_v4();
        add_user(&t.db, id, "alice", false).await;
        let user = get_user_by_id(&t.db, id).await.unwrap().unwrap();
        assert_eq!((user.id, user.username.as_str()), (id, "alice"));
    }

    #[tokio::test]
    async fn get_admin_user_returns_admin_or_none() {
        let t = TestDb::new().await;
        assert!(get_admin_user(&t.db).await.unwrap().is_none());
        add_user(&t.db, Uuid::new_v4(), "alice", false).await;
        assert!(get_admin_user(&t.db).await.unwrap().is_none());
        let admin_id = Uuid::new_v4();
        add_user(&t.db, admin_id, "admin", true).await;
        let admin = get_admin_user(&t.db).await.unwrap().unwrap();
        assert_eq!(admin.id, admin_id);
        assert!(admin.is_admin);
    }
}
