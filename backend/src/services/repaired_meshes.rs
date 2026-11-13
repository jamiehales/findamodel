//! Repaired model meshes ([`crate::geometry::watertight::repair_model`]) kept between repairs, so
//! a mesh is repaired once rather than on every remeshed download or plate export.
//!
//! Entries are files (like support exports), named by the key of the repair
//! ([`watertight::mesh_key`], whatever thread count it was repaired on) and the repair version, so
//! a new repair version misses every entry. The file is the authority: its header holds the version and the key, and an entry is
//! used only when both are current and it reads back whole; anything else is repaired again and
//! rewritten. The `model_repaired_meshes` table indexes which mesh each model's repair was made
//! from (and what the mesh was made from) for the scan to find entries no model uses any more.
//! Entries are kept apart from the VOXL download cache. Each file's modified time is when it was
//! last read or written; when the store grows past its size limit the entries read longest ago
//! are deleted (after each write, and at the sweep).

pub mod codec;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use tracing::warn;
use uuid::Uuid;

use crate::db::Db;
use crate::entities::{ModelRepairedMesh, ModelRow};
use crate::geometry::Triangle;
use crate::geometry::Vec3;
use crate::geometry::hull::DEFAULT_RAFT_HEIGHT_MM;
use crate::geometry::watertight::{self, CURRENT_REPAIR_VERSION, MeshKey, RepairStore};
use crate::services::scan_config;
pub use codec::Header;

const LOG_TARGET: &str = "findamodel.RepairedMeshes";
const EXTENSION: &str = "repair";
/// Files no model indexes are swept only once they are this old: a repair writes its entry
/// before indexing it, and a scan's sweep must not take a file between the two.
pub const SWEEP_GRACE: Duration = Duration::from_secs(60 * 60);

/// The entry file's path under the store for the mesh with `key`, at the current repair version,
/// relative.
pub fn relative_path(key: &MeshKey) -> String {
    let hex = hex::encode(key);
    format!("{}/{hex}.v{CURRENT_REPAIR_VERSION}.{EXTENSION}", &hex[..2])
}

/// What the mesh `model`'s repair was made from depends on: its file, and for a pre-supported
/// model how its supports were split off.
pub fn source(model: &ModelRow) -> String {
    source_of(&model.checksum, model.hull_raft_height_mm, model.calculated_supported)
}

fn source_of(checksum: &str, raft_height_mm: Option<f32>, supported: Option<bool>) -> String {
    let scan_config = scan_config::compute(raft_height_mm.unwrap_or(DEFAULT_RAFT_HEIGHT_MM));
    format!("{checksum}|{scan_config}|supported:{supported:?}")
}

/// An index row with what its model is made from now: model id, mesh key, stored source, and the
/// model's checksum, raft height and supported metadata.
type IndexedRow = (Uuid, String, String, String, Option<f32>, Option<bool>);

#[derive(Clone)]
pub struct RepairedMeshes {
    inner: Arc<Inner>,
}

struct Inner {
    dir: PathBuf,
    /// The size limit in bytes (0 = nothing is kept), shared with the settings so a change
    /// applies at the next eviction.
    max_bytes: Arc<AtomicU64>,
    /// Held while evicting, so two writers don't both delete to make room.
    evicting: parking_lot::Mutex<()>,
    /// Repairs read from an entry.
    hits: AtomicUsize,
    /// Entries written.
    writes: AtomicUsize,
}

impl RepairedMeshes {
    /// A store in `dir` holding up to `max_bytes` of entries (0 = nothing is kept), read each
    /// time the store evicts (see [`crate::services::app_config::AppConfigService::repaired_mesh_cache_bytes`]).
    pub fn new(dir: PathBuf, max_bytes: Arc<AtomicU64>) -> Self {
        RepairedMeshes {
            inner: Arc::new(Inner {
                dir,
                max_bytes,
                evicting: parking_lot::Mutex::new(()),
                hits: AtomicUsize::new(0),
                writes: AtomicUsize::new(0),
            }),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.inner.dir
    }

    fn max_bytes(&self) -> u64 {
        self.inner.max_bytes.load(Ordering::Relaxed)
    }

    /// How many repairs so far were read from an entry instead of being run.
    pub fn hits(&self) -> usize {
        self.inner.hits.load(Ordering::Relaxed)
    }

    /// How many entries so far were written.
    pub fn writes(&self) -> usize {
        self.inner.writes.load(Ordering::Relaxed)
    }

    /// The repaired mesh stored for `key`, when its entry is whole and at the current repair
    /// version, marked as just read. Blocking.
    pub fn read(&self, key: &MeshKey) -> Option<Vec<Triangle>> {
        self.read_at(key, SystemTime::now())
    }

    fn read_at(&self, key: &MeshKey, now: SystemTime) -> Option<Vec<Triangle>> {
        if self.max_bytes() == 0 {
            return None;
        }
        let relative = relative_path(key);
        let path = self.inner.dir.join(&relative);
        // Evicted entries vanish between listing and reading; any failure to read is a miss.
        let bytes = fs::read(&path).ok()?;
        let current = Header { repair_version: CURRENT_REPAIR_VERSION, key: *key };
        match codec::decode(&bytes) {
            Some((header, triangles)) if header == current => {
                if let Err(err) = touch(&path, now) {
                    warn!(target: LOG_TARGET, "Failed to mark repaired mesh {relative} as read: {err}");
                }
                Some(triangles)
            }
            _ => {
                warn!(target: LOG_TARGET, "Repaired mesh {relative} is damaged or not this mesh's; repairing again");
                None
            }
        }
    }

    /// Stores `repaired` as the repair of the mesh with `key`, replacing any entry there, then
    /// evicts the entries read longest ago until the store fits its limit. Returns the entry's
    /// relative path, or None when it is not kept (no store, or bigger than the whole store).
    /// Blocking.
    pub fn write(&self, key: &MeshKey, repaired: &[Triangle]) -> std::io::Result<Option<String>> {
        self.write_at(key, repaired, SystemTime::now())
    }

    fn write_at(&self, key: &MeshKey, repaired: &[Triangle], now: SystemTime) -> std::io::Result<Option<String>> {
        // The file keeps no normals; a mesh with some would not read back as it was.
        if repaired.iter().any(|t| t.normal != Vec3::default()) {
            return Err(std::io::Error::other("a repaired mesh carries no normals"));
        }
        if self.max_bytes() == 0 {
            return Ok(None);
        }
        let bytes = codec::encode(&Header { repair_version: CURRENT_REPAIR_VERSION, key: *key }, repaired)?;
        if bytes.len() as u64 > self.max_bytes() {
            return Ok(None);
        }
        let relative = relative_path(key);
        let path = self.inner.dir.join(&relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Written beside the entry and renamed into place, so readers never see part of it (and
        // eviction, which counts only whole entries, never takes it).
        let partial = path.with_extension(format!("{}.partial", Uuid::new_v4().simple()));
        let written = fs::write(&partial, bytes).and_then(|()| touch(&partial, now));
        if let Err(err) = written.and_then(|()| fs::rename(&partial, &path)) {
            let _ = fs::remove_file(&partial);
            return Err(err);
        }
        self.inner.writes.fetch_add(1, Ordering::Relaxed);
        if let Err(err) = self.evict() {
            warn!(target: LOG_TARGET, "Failed to evict repaired meshes from {}: {err}", self.inner.dir.display());
        }
        Ok(Some(relative))
    }

    /// Deletes the entries read longest ago until the rest fit the limit. Returns how many were
    /// deleted. A model whose entry goes is repaired again when next asked for (its index row
    /// stays, and is dropped by the next sweep).
    fn evict(&self) -> std::io::Result<usize> {
        let _evicting = self.inner.evicting.lock();
        let max_bytes = self.max_bytes();
        let mut entries = Vec::new();
        let Ok(dirs) = fs::read_dir(&self.inner.dir) else { return Ok(0) };
        for dir in dirs.flatten().filter(|d| d.path().is_dir()) {
            for file in fs::read_dir(dir.path())?.flatten() {
                if file.path().extension().is_none_or(|e| e != EXTENSION) {
                    continue;
                }
                match file.metadata() {
                    Ok(meta) => entries.push((meta.modified()?, meta.len(), file.path())),
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => return Err(err),
                }
            }
        }
        let mut total: u64 = entries.iter().map(|(_, len, _)| len).sum();
        let mut removed = 0;
        entries.sort_by_key(|(accessed, _, _)| *accessed);
        for (_, len, path) in entries {
            if total <= max_bytes {
                break;
            }
            match fs::remove_file(&path) {
                Ok(()) => removed += 1,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
            total -= len;
        }
        Ok(removed)
    }

    /// Records that `model`'s repair was made from the mesh with `key`, and deletes the entry its
    /// index named before when no other model uses it.
    pub async fn index(&self, db: &Db, model: &ModelRow, key: &MeshKey) -> sqlx::Result<()> {
        let mesh_key = hex::encode(key);
        let previous: Option<String> =
            sqlx::query_scalar("SELECT mesh_key FROM model_repaired_meshes WHERE model_id = ?")
                .bind(model.id)
                .fetch_optional(db)
                .await?;
        sqlx::query(
            "INSERT INTO model_repaired_meshes (model_id, mesh_key, source) VALUES (?, ?, ?)
             ON CONFLICT(model_id) DO UPDATE SET mesh_key = excluded.mesh_key, source = excluded.source",
        )
        .bind(model.id)
        .bind(&mesh_key)
        .bind(source(model))
        .execute(db)
        .await?;
        if let Some(previous) = previous.filter(|p| *p != mesh_key) {
            self.release(db, &previous).await;
        }
        Ok(())
    }

    /// [`RepairedMeshes::index`], logging a failure instead of returning it.
    pub async fn index_or_warn(&self, db: &Db, model: &ModelRow, key: &MeshKey) {
        if let Err(err) = self.index(db, model, key).await {
            warn!(target: LOG_TARGET, "Failed to index the repaired mesh of {}: {err}", model.id);
        }
    }

    /// Deletes the entry for the mesh with key `mesh_key` (hex) when no model indexes it any
    /// more. A file a repair has open stays readable to it.
    async fn release(&self, db: &Db, mesh_key: &str) {
        let used: sqlx::Result<bool> =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM model_repaired_meshes WHERE mesh_key = ?)")
                .bind(mesh_key)
                .fetch_one(db)
                .await;
        match used {
            Ok(false) => {
                if let Some(key) = parse_key(mesh_key) {
                    self.remove(&relative_path(&key));
                }
            }
            Ok(true) => {}
            Err(err) => warn!(target: LOG_TARGET, "Failed to check who uses repaired mesh {mesh_key}: {err}"),
        }
    }

    /// Drops the index rows whose model has changed since its repair, then deletes the entry
    /// files no row names at the current repair version (entries of an older version, of changed
    /// or removed models), of those last written before `grace` ago (see [`SWEEP_GRACE`]), and
    /// left-over partial files as old; then evicts down to the size limit and drops the rows
    /// whose entry is gone. Returns how many files were deleted. Run at the end of a full scan
    /// only, when every model row is up to date.
    pub async fn sweep(&self, db: &Db, grace: Duration) -> usize {
        match sqlx::query_scalar::<_, i64>("SELECT repaired_mesh_cache_size_mb FROM app_config WHERE id = 1")
            .fetch_optional(db)
            .await
        {
            Ok(Some(mb)) => self.inner.max_bytes.store(mb.max(1) as u64 * 1024 * 1024, Ordering::Relaxed),
            Ok(None) => {}
            Err(err) => warn!(target: LOG_TARGET, "Failed to read the repaired mesh cache size: {err}"),
        }
        let rows: Vec<IndexedRow> = match sqlx::query_as(
            "SELECT r.model_id, r.mesh_key, r.source, m.checksum, m.hull_raft_height_mm, m.calculated_supported
             FROM model_repaired_meshes r JOIN models m ON m.id = r.model_id",
        )
        .fetch_all(db)
        .await
        {
            Ok(rows) => rows,
            Err(err) => {
                warn!(target: LOG_TARGET, "Failed to list indexed repaired meshes: {err}");
                return 0;
            }
        };
        let mut indexed: Vec<(Uuid, String)> = Vec::new();
        let mut stale = Vec::new();
        for (model_id, mesh_key, stored, checksum, raft, supported) in rows {
            match parse_key(&mesh_key).filter(|_| stored == source_of(&checksum, raft, supported)) {
                Some(key) => indexed.push((model_id, relative_path(&key))),
                None => stale.push(model_id),
            }
        }
        let store = self.clone();
        let (removed, gone) = tokio::task::spawn_blocking(move || {
            let named: std::collections::HashSet<&str> = indexed.iter().map(|(_, r)| r.as_str()).collect();
            let cutoff = SystemTime::now().checked_sub(grace).unwrap_or(SystemTime::UNIX_EPOCH);
            let mut removed = 0;
            if let Ok(dirs) = fs::read_dir(store.dir()) {
                for dir in dirs.flatten().filter(|d| d.path().is_dir()) {
                    let Ok(files) = fs::read_dir(dir.path()) else { continue };
                    for file in files.flatten() {
                        let relative =
                            format!("{}/{}", dir.file_name().to_string_lossy(), file.file_name().to_string_lossy());
                        let old = file.metadata().and_then(|m| m.modified()).is_ok_and(|m| m < cutoff);
                        if old && !named.contains(relative.as_str()) && store.remove(&relative) {
                            removed += 1;
                        }
                    }
                }
            }
            match store.evict() {
                Ok(evicted) => removed += evicted,
                Err(err) => warn!(target: LOG_TARGET, "Failed to evict repaired meshes: {err}"),
            }
            let gone: Vec<Uuid> =
                indexed.iter().filter(|(_, r)| !store.dir().join(r).exists()).map(|(id, _)| *id).collect();
            (removed, gone)
        })
        .await
        .unwrap_or_default();
        for model_id in stale.into_iter().chain(gone) {
            let dropped =
                sqlx::query("DELETE FROM model_repaired_meshes WHERE model_id = ?").bind(model_id).execute(db);
            if let Err(err) = dropped.await {
                warn!(target: LOG_TARGET, "Failed to drop the repaired mesh index of {model_id}: {err}");
                break;
            }
        }
        removed
    }

    /// Deletes one file under the store; false when it could not be deleted.
    fn remove(&self, relative: &str) -> bool {
        match fs::remove_file(self.inner.dir.join(relative)) {
            Ok(()) => true,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
            Err(err) => {
                warn!(target: LOG_TARGET, "Failed to delete repaired mesh {relative}: {err}");
                false
            }
        }
    }
}

impl RepairStore for RepairedMeshes {
    fn load(&self, key: &MeshKey) -> Option<Vec<Triangle>> {
        let triangles = self.read(key)?;
        self.inner.hits.fetch_add(1, Ordering::Relaxed);
        Some(triangles)
    }

    fn save(&self, key: &MeshKey, repaired: &[Triangle]) {
        if let Err(err) = self.write(key, repaired) {
            warn!(target: LOG_TARGET, "Failed to store repaired mesh {}: {err}", relative_path(key));
        }
    }
}

/// Sets `path`'s modified time, which records when the entry was last read or written.
fn touch(path: &Path, at: SystemTime) -> std::io::Result<()> {
    fs::File::options().write(true).open(path)?.set_modified(at)
}

/// The index row of `model_id`'s repaired mesh, if it has one.
pub async fn indexed(db: &Db, model_id: Uuid) -> sqlx::Result<Option<ModelRepairedMesh>> {
    sqlx::query_as("SELECT model_id, mesh_key, source FROM model_repaired_meshes WHERE model_id = ?")
        .bind(model_id)
        .fetch_optional(db)
        .await
}

fn parse_key(hex_key: &str) -> Option<MeshKey> {
    hex::decode(hex_key).ok()?.try_into().ok()
}

/// [`watertight::repair_model`] on `threads` threads with `store` (when there is one) as its store.
pub fn repair(triangles: &[Triangle], store: Option<&RepairedMeshes>, threads: usize) -> watertight::Repair {
    watertight::repair_model(triangles, store.map(|s| s as &dyn RepairStore), threads)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh(n: usize, at: f32) -> Vec<Triangle> {
        (0..n)
            .map(|i| {
                let z = at + i as f32;
                Triangle::new(Vec3::new(0.0, z, 0.0), Vec3::new(1.0, z, 0.0), Vec3::new(0.0, z, 1.0), Vec3::default())
            })
            .collect()
    }

    /// A store whose limit is never reached.
    fn roomy(dir: PathBuf) -> RepairedMeshes {
        RepairedMeshes::new(dir, Arc::new(AtomicU64::new(1 << 40)))
    }

    /// A closed 10 mm cube at x = `x` (no other test repairs one there).
    fn cube(x: f32) -> Vec<Triangle> {
        let p = |i: usize| Vec3::new(x + 10.0 * (i & 1) as f32, 10.0 * ((i >> 1) & 1) as f32, 10.0 * (i >> 2) as f32);
        let faces = [
            [0, 2, 1],
            [1, 2, 3],
            [4, 5, 6],
            [5, 7, 6],
            [0, 1, 4],
            [1, 5, 4],
            [2, 6, 3],
            [3, 6, 7],
            [0, 4, 2],
            [2, 4, 6],
            [1, 3, 5],
            [3, 7, 5],
        ];
        faces.iter().map(|f| Triangle::new(p(f[0]), p(f[1]), p(f[2]), Vec3::default())).collect()
    }

    #[test]
    fn an_entry_is_read_back_bit_for_bit_and_counted() {
        let dir = tempfile::tempdir().unwrap();
        let store = roomy(dir.path().to_path_buf());
        let key = [1u8; 32];
        let repaired = mesh(5, -0.0);
        assert_eq!(store.load(&key), None, "a miss");
        store.save(&key, &repaired);
        assert_eq!(store.writes(), 1);
        let read = store.load(&key).unwrap();
        let bits = |t: &[Triangle]| bytemuck::cast_slice::<Triangle, u32>(t).to_vec();
        assert_eq!(bits(&read), bits(&repaired));
        assert_eq!(store.hits(), 1);
        assert_eq!(store.load(&[2u8; 32]), None, "another mesh");
        let relative = relative_path(&key);
        assert!(relative.ends_with(&format!(".v{CURRENT_REPAIR_VERSION}.repair")), "{relative}");
        assert_eq!(fs::read_dir(dir.path().join(&relative[..2])).unwrap().count(), 1, "no partial files left");
    }

    #[test]
    fn a_damaged_entry_or_one_of_another_version_or_mesh_is_repaired_again() {
        let dir = tempfile::tempdir().unwrap();
        let store = roomy(dir.path().to_path_buf());
        let key = [1u8; 32];
        let path = dir.path().join(relative_path(&key));
        let put = |header: Header| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, codec::encode(&header, &mesh(3, 0.0)).unwrap()).unwrap();
        };
        put(Header { repair_version: CURRENT_REPAIR_VERSION - 1, key });
        assert_eq!(store.read(&key), None, "another repair version");
        put(Header { repair_version: CURRENT_REPAIR_VERSION, key: [9; 32] });
        assert_eq!(store.read(&key), None, "another mesh's");
        put(Header { repair_version: CURRENT_REPAIR_VERSION, key });
        assert!(store.read(&key).is_some());
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, &bytes[..bytes.len() - 2]).unwrap();
        assert_eq!(store.read(&key), None, "a cut entry");
        fs::write(&path, b"not an entry").unwrap();
        assert_eq!(store.read(&key), None);
        assert_eq!(store.hits(), 0, "only the whole one would count");

        // A repair over a damaged entry repairs, and rewrites it.
        let cube = cube(41.0);
        let k = watertight::mesh_key(&cube);
        let at = dir.path().join(relative_path(&k));
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(&at, b"damaged").unwrap();
        let fresh = repair(&cube, Some(&store), 2);
        assert_eq!(fresh.triangles.len(), 12);
        assert_eq!((store.hits(), store.writes()), (0, 1));
        let again = repair(&cube, Some(&store), 2);
        assert_eq!((store.hits(), store.writes()), (1, 1));
        assert_eq!(again.triangles, fresh.triangles);
    }

    #[test]
    fn a_mesh_with_normals_is_not_stored() {
        let dir = tempfile::tempdir().unwrap();
        let store = roomy(dir.path().to_path_buf());
        let mut m = mesh(2, 0.0);
        m[1].normal = Vec3::new(0.0, 0.0, 1.0);
        assert!(store.write(&[1; 32], &m).is_err());
        assert_eq!(store.read(&[1; 32]), None);
    }

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + seconds)
    }

    /// Entries for meshes 1, 2, ... of one size, and that size.
    fn entry(i: u8) -> (MeshKey, Vec<Triangle>) {
        ([i; 32], mesh(200, f32::from(i) * 1000.0))
    }

    fn entry_size() -> u64 {
        let (key, m) = entry(1);
        codec::encode(&Header { repair_version: CURRENT_REPAIR_VERSION, key }, &m).unwrap().len() as u64
    }

    fn kept(store: &RepairedMeshes, i: u8) -> bool {
        store.dir().join(relative_path(&entry(i).0)).exists()
    }

    #[test]
    fn the_entries_read_longest_ago_are_evicted_to_fit_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let limit = Arc::new(AtomicU64::new(entry_size() * 5 / 2));
        let store = RepairedMeshes::new(dir.path().to_path_buf(), limit.clone());
        let write = |i: u8, t: u64| {
            let (key, m) = entry(i);
            store.write_at(&key, &m, at(t)).unwrap().unwrap();
        };
        write(1, 0);
        write(2, 1);
        // 1 is read after 2 was written: 2 is the one read longest ago when 3 comes.
        assert!(store.read_at(&entry(1).0, at(2)).is_some());
        write(3, 3);
        assert!(kept(&store, 1) && !kept(&store, 2) && kept(&store, 3));
        assert!(store.read(&entry(2).0).is_none(), "an evicted entry is a miss");

        // A partial file (an entry being written) is never taken.
        let partial = dir.path().join(format!("{}.0.partial", relative_path(&[9; 32])));
        fs::create_dir_all(partial.parent().unwrap()).unwrap();
        fs::write(&partial, vec![0u8; entry_size() as usize * 4]).unwrap();
        // A smaller limit applies at the next write: only the newest entry fits.
        limit.store(entry_size() * 3 / 2, Ordering::Relaxed);
        write(4, 4);
        assert!(!kept(&store, 1) && !kept(&store, 3) && kept(&store, 4));
        assert!(partial.exists());
    }

    #[test]
    fn an_entry_bigger_than_the_store_or_a_zero_limit_keeps_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (key, m) = entry(1);
        let small = RepairedMeshes::new(dir.path().join("small"), Arc::new(AtomicU64::new(entry_size() - 1)));
        assert_eq!(small.write(&key, &m).unwrap(), None);
        assert!(small.read(&key).is_none());
        let off = RepairedMeshes::new(dir.path().join("off"), Arc::new(AtomicU64::new(0)));
        assert_eq!(off.write(&key, &m).unwrap(), None);
        assert!(!dir.path().join("off").exists());
    }

    #[tokio::test]
    async fn the_sweep_evicts_to_the_settings_limit_and_drops_the_rows_of_evicted_entries() {
        let t = crate::test_support::TestDb::new().await;
        let dir = tempfile::tempdir().unwrap();
        let store = roomy(dir.path().to_path_buf());
        let (a, b) = (crate::test_support::model("a.stl", ""), crate::test_support::model("b.stl", ""));
        t.insert_model(&a).await;
        t.insert_model(&b).await;
        // Two entries of about 0.7 MB each (corners that do not compress), b read last.
        let noisy = |seed: u32| -> Vec<Triangle> {
            let mut x = seed;
            let mut next = || {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                f32::from_bits((x >> 9) | 0x3f80_0000)
            };
            (0..20_000)
                .map(|_| {
                    Triangle::new(
                        Vec3::new(next(), next(), next()),
                        Vec3::new(next(), next(), next()),
                        Vec3::new(next(), next(), next()),
                        Vec3::default(),
                    )
                })
                .collect()
        };
        for (i, model, time) in [(1u8, &a, 0), (2, &b, 1)] {
            store.write_at(&[i; 32], &noisy(u32::from(i)), at(time)).unwrap().unwrap();
            store.index(&t.db, model, &[i; 32]).await.unwrap();
        }
        let size = |i: u8| fs::metadata(store.dir().join(relative_path(&[i; 32]))).unwrap().len();
        assert!(size(1) + size(2) > 1024 * 1024 && size(2) < 1024 * 1024, "{} {}", size(1), size(2));
        // The settings allow 1 MB: the sweep takes the limit from them and evicts a's entry.
        sqlx::query("INSERT INTO app_config (id, repaired_mesh_cache_size_mb, updated_at) VALUES (1, 1, ?)")
            .bind(crate::db::now())
            .execute(&t.db)
            .await
            .unwrap();
        assert_eq!(store.sweep(&t.db, SWEEP_GRACE).await, 1);
        assert_eq!(store.max_bytes(), 1024 * 1024);
        assert!(!store.dir().join(relative_path(&[1; 32])).exists());
        assert!(store.dir().join(relative_path(&[2; 32])).exists());
        assert!(indexed(&t.db, a.id).await.unwrap().is_none(), "the evicted entry's row is dropped");
        assert!(indexed(&t.db, b.id).await.unwrap().is_some());
    }

    fn aged(store: &RepairedMeshes, relative: &str, age: Duration) -> PathBuf {
        let path = store.dir().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"entry").unwrap();
        fs::File::options().write(true).open(&path).unwrap().set_modified(SystemTime::now() - age).unwrap();
        path
    }

    #[tokio::test]
    async fn indexing_another_mesh_releases_the_old_entry_when_no_model_uses_it() {
        let t = crate::test_support::TestDb::new().await;
        let dir = tempfile::tempdir().unwrap();
        let store = roomy(dir.path().to_path_buf());
        let (a, b) = (crate::test_support::model("a.stl", ""), crate::test_support::model("b.stl", ""));
        t.insert_model(&a).await;
        t.insert_model(&b).await;
        let (first, second) = ([1u8; 32], [2u8; 32]);
        let first_file = aged(&store, &relative_path(&first), Duration::ZERO);
        store.index(&t.db, &a, &first).await.unwrap();
        store.index(&t.db, &b, &first).await.unwrap();
        assert_eq!(indexed(&t.db, a.id).await.unwrap().unwrap().mesh_key, hex::encode(first));
        store.index(&t.db, &a, &second).await.unwrap();
        assert!(first_file.exists(), "b still uses it");
        store.index(&t.db, &b, &second).await.unwrap();
        assert!(!first_file.exists());
        let row = indexed(&t.db, b.id).await.unwrap().unwrap();
        assert_eq!((row.mesh_key, row.source), (hex::encode(second), source(&b)));
    }

    #[tokio::test]
    async fn the_sweep_deletes_old_entries_no_current_model_uses() {
        let t = crate::test_support::TestDb::new().await;
        let dir = tempfile::tempdir().unwrap();
        let store = roomy(dir.path().to_path_buf());
        let hour = Duration::from_secs(3600);
        let (used, changed, orphan, young) = ([1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32]);
        let used_file = aged(&store, &relative_path(&used), 3 * hour);
        let changed_file = aged(&store, &relative_path(&changed), 3 * hour);
        let orphan_file = aged(&store, &relative_path(&orphan), 3 * hour);
        let young_file = aged(&store, &relative_path(&young), Duration::from_secs(60));
        // The used mesh's entry from before a repair version change.
        let hex = hex::encode(used);
        let old_version =
            aged(&store, &format!("{}/{hex}.v{}.repair", &hex[..2], CURRENT_REPAIR_VERSION - 1), 3 * hour);
        let partial = aged(&store, &format!("{}/{hex}.v2.1234.partial", &hex[..2]), 3 * hour);

        let a = crate::test_support::model("a.stl", "");
        let mut b = crate::test_support::model("b.stl", "");
        t.insert_model(&a).await;
        t.insert_model(&b).await;
        store.index(&t.db, &a, &used).await.unwrap();
        store.index(&t.db, &b, &changed).await.unwrap();
        // b's file changes after its repair: its index is stale.
        b.checksum = "edited".into();
        t.insert_model(&b).await;

        assert_eq!(store.sweep(&t.db, SWEEP_GRACE).await, 4);
        assert!(used_file.exists());
        assert!(young_file.exists(), "written too recently: a repair may be about to index it");
        for gone in [&changed_file, &orphan_file, &old_version, &partial] {
            assert!(!gone.exists(), "{}", gone.display());
        }
        assert!(indexed(&t.db, a.id).await.unwrap().is_some());
        assert!(indexed(&t.db, b.id).await.unwrap().is_none(), "the stale row is dropped");
        assert_eq!(store.sweep(&t.db, SWEEP_GRACE).await, 0);
        let missing = roomy(dir.path().join("none"));
        assert_eq!(missing.sweep(&t.db, SWEEP_GRACE).await, 0);
    }
}
