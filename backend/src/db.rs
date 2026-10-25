//! SQLite connection pool, migrations and startup lock checks.

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, SubsecRound, Utc};
use log::LevelFilter;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{ConnectOptions, Connection, SqlitePool};
use tracing::{info, warn};

pub type Db = SqlitePool;

/// Statements slower than this are logged as warnings with their SQL, so a stalled
/// connection pool can be traced to the query holding it.
const SLOW_STATEMENT_THRESHOLD: Duration = Duration::from_secs(1);

/// Current time at microsecond precision (what is stored and serialised).
pub fn now() -> DateTime<Utc> {
    Utc::now().trunc_subsecs(6)
}

fn connect_options(path: &Path) -> SqliteConnectOptions {
    SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))
        .expect("sqlite url")
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(10))
        .foreign_keys(true)
        .log_statements(LevelFilter::Off)
        .log_slow_statements(LevelFilter::Warn, SLOW_STATEMENT_THRESHOLD)
}

/// Opens (creating if needed) the database and applies migrations.
pub async fn open(path: &Path) -> Result<Db> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .acquire_timeout(Duration::from_secs(60))
        .connect_with(connect_options(path))
        .await
        .with_context(|| format!("opening database {}", path.display()))?;

    info!(target: "findamodel.Startup", "Starting database migration for {}", path.display());
    let compact = !migration_applied(&pool, HULL_SPLIT_MIGRATION).await;
    tokio::time::timeout(MIGRATION_TIMEOUT, sqlx::migrate!("./migrations").run(&pool))
        .await
        .with_context(|| {
            format!(
                "Database migration timed out after {} seconds for {}. This is usually caused by a locked or busy SQLite database.",
                MIGRATION_TIMEOUT.as_secs(),
                path.display()
            )
        })?
        .context("running database migrations")?;
    info!(target: "findamodel.Startup", "Database migration complete for {}", path.display());

    if compact && migration_applied(&pool, HULL_SPLIT_MIGRATION).await {
        info!(target: "findamodel.Startup", "Compacting {} after moving hulls out of models", path.display());
        let started = std::time::Instant::now();
        sqlx::query("VACUUM").execute(&pool).await.context("compacting the database")?;
        info!(target: "findamodel.Startup", "Database compacted in {:.1}s", started.elapsed().as_secs_f64());
    }

    Ok(pool)
}

/// Generous because the hull split rewrites the whole models table once.
const MIGRATION_TIMEOUT: Duration = Duration::from_secs(120);

/// The migration that moves hulls into `model_hulls`. The startup that applies it compacts the
/// file afterwards, since sqlx runs every SQLite migration in a transaction and VACUUM can't
/// run in one.
const HULL_SPLIT_MIGRATION: i64 = 6;

async fn migration_applied(pool: &Db, version: i64) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM _sqlx_migrations WHERE version = ? AND success)")
        .bind(version)
        .fetch_one(pool)
        .await
        .unwrap_or(false)
}

async fn is_locked(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .busy_timeout(Duration::from_millis(250))
        .disable_statement_logging();
    let mut conn = match options.connect().await {
        Ok(c) => c,
        Err(err) => return Ok(Some(err.to_string())),
    };
    let result = sqlx::raw_sql("BEGIN IMMEDIATE; ROLLBACK;").execute(&mut conn).await;
    let _ = conn.close().await;
    match result {
        Ok(_) => Ok(None),
        Err(sqlx::Error::Database(db)) if db.message().contains("locked") || db.message().contains("busy") => {
            Ok(Some(db.message().to_string()))
        }
        Err(err) => Err(err.into()),
    }
}

fn lock_holder_pids(path: &Path) -> Vec<u32> {
    let candidates: Vec<String> = [path.to_path_buf(), path.with_extension("db-wal"), path.with_extension("db-shm")]
        .iter()
        .filter(|p| p.exists())
        .map(|p| p.display().to_string())
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    let lsof = if Path::new("/usr/sbin/lsof").exists() { "/usr/sbin/lsof" } else { "lsof" };
    let Ok(output) = std::process::Command::new(lsof).arg("-t").args(&candidates).output() else {
        return Vec::new();
    };
    let mut pids: Vec<u32> =
        String::from_utf8_lossy(&output.stdout).lines().filter_map(|l| l.trim().parse().ok()).collect();
    pids.sort_unstable();
    pids.dedup();
    pids
}

/// Fails fast when another process holds the database lock. Stale holders (e.g. a
/// previous instance that did not exit) are terminated first.
pub async fn ensure_unlocked(path: &Path) -> Result<()> {
    let Some(initial_error) = is_locked(path).await? else {
        return Ok(());
    };

    let holders = lock_holder_pids(path);
    let current = std::process::id();
    let mut killed = false;
    for pid in holders.iter().copied().filter(|p| *p != current) {
        let status = std::process::Command::new("kill").arg("-9").arg(pid.to_string()).status();
        match status {
            Ok(s) if s.success() => {
                warn!(target: "findamodel.Startup", "Killed process {pid} while clearing SQLite lock on {}", path.display());
                killed = true;
            }
            _ => {
                warn!(target: "findamodel.Startup", "Failed to kill process {pid} while clearing SQLite lock on {}", path.display())
            }
        }
    }
    if killed {
        tokio::time::sleep(Duration::from_millis(400)).await;
    }

    match is_locked(path).await? {
        None => {
            warn!(target: "findamodel.Startup", "SQLite lock cleared for {}. Startup continuing.", path.display());
            Ok(())
        }
        Some(retry_error) => {
            let pids = if holders.is_empty() {
                "unknown".to_string()
            } else {
                holders.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
            };
            bail!(
                "Database file is locked and startup cannot continue. DbPath: {}. Lock holders: {pids}. Lock error: {}",
                path.display(),
                if retry_error.is_empty() { initial_error } else { retry_error }
            )
        }
    }
}

/// Splits values into chunks small enough for SQLite's bound-parameter limit.
pub fn param_chunks<T>(values: &[T]) -> std::slice::Chunks<'_, T> {
    values.chunks(500)
}

/// `?, ?, ?` with `n` placeholders.
pub fn placeholders(n: usize) -> String {
    vec!["?"; n].join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    // sqlx logs statements from the connection's worker thread, out of reach of a test
    // subscriber, so this checks the settings the pool connects with.
    #[test]
    fn only_slow_statements_are_logged() {
        let options = format!("{:?}", connect_options(Path::new("findamodel.db")));
        assert!(options.contains("statements_level: Off"), "{options}");
        assert!(options.contains("slow_statements_level: Warn"), "{options}");
        assert!(options.contains("slow_statements_duration: 1s"), "{options}");
    }

    #[tokio::test]
    async fn hull_split_moves_hulls_and_compacts_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("findamodel.db");
        let mut migrator = sqlx::migrate!("./migrations");
        migrator.migrations =
            migrator.migrations.iter().filter(|m| m.version < HULL_SPLIT_MIGRATION).cloned().collect::<Vec<_>>().into();
        let before = SqlitePoolOptions::new().connect_with(connect_options(&path)).await.unwrap();
        migrator.run(&before).await.unwrap();
        let hull = format!("[{}]", vec!["[1.5,2.5]"; 20_000].join(","));
        for i in 0..20 {
            sqlx::query(
                "INSERT INTO models (id, checksum, file_name, directory, file_type, file_size, file_modified_at,
                    cached_at, convex_hull, concave_hull) VALUES (?, 'c', ?, '', 'stl', 1, ?, ?, ?, ?)",
            )
            .bind(Uuid::new_v4())
            .bind(format!("m{i}.stl"))
            .bind(now())
            .bind(now())
            .bind(&hull)
            .bind((i % 2 == 0).then_some(&hull))
            .execute(&before)
            .await
            .unwrap();
        }
        sqlx::query("INSERT INTO models (id, checksum, file_name, directory, file_type, file_size, file_modified_at, cached_at) VALUES (?, 'c', 'bare.ctb', '', 'ctb', 1, ?, ?)")
            .bind(Uuid::new_v4())
            .bind(now())
            .bind(now())
            .execute(&before)
            .await
            .unwrap();
        before.close().await;

        let db = open(&path).await.unwrap();
        let counts: (i64, i64) =
            sqlx::query_as("SELECT COUNT(*), COUNT(concave_hull) FROM model_hulls").fetch_one(&db).await.unwrap();
        assert_eq!(counts, (20, 10));
        let stored: String =
            sqlx::query_scalar("SELECT convex_hull FROM model_hulls LIMIT 1").fetch_one(&db).await.unwrap();
        assert_eq!(stored, hull);
        let model_columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('models')").fetch_all(&db).await.unwrap();
        for dropped in ["convex_hull", "concave_hull", "convex_sans_raft_hull"] {
            assert!(!model_columns.iter().any(|c| c == dropped), "{model_columns:?}");
        }
        let free_pages: i64 = sqlx::query_scalar("PRAGMA freelist_count").fetch_one(&db).await.unwrap();
        assert_eq!(free_pages, 0);
    }
}
