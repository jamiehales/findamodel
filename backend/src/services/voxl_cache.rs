//! On-disk cache of generated model VOXLs (`GET /api/models/{id}/voxl`), so a model's
//! supports are split off and rebuilt once rather than on every download.
//!
//! Entries are files named by a hash of their [`VoxlCacheKey`]: a change to the model file,
//! its raft height, the support detection or the support layout gives a new key, and the old
//! entry is never read again. Each file's modified time is when it was last read or written;
//! when the cache grows past its size limit the entries read longest ago are deleted.

use std::fs::{self, File};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use tracing::warn;
use uuid::Uuid;

use crate::entities::ModelRow;
use crate::geometry::support_layout::CURRENT_SUPPORT_LAYOUT_VERSION;
use crate::services::scan_config;

const LOG_TARGET: &str = "findamodel.VoxlCache";
const ENTRY_EXTENSION: &str = "voxl";
const PARTIAL_EXTENSION: &str = "partial";

/// Everything a model VOXL is generated from.
#[derive(Debug, Clone, PartialEq)]
pub struct VoxlCacheKey {
    /// The model file's content checksum.
    pub checksum: String,
    /// Written into the VOXL as the model's name.
    pub file_name: String,
    /// Splits the supports from the model (with the support detection version, see
    /// [`scan_config::compute`]).
    pub raft_height_mm: f32,
    /// The model's supported metadata, which decides whether an unsure split is used.
    pub supported: Option<bool>,
    pub layout_version: i32,
}

impl VoxlCacheKey {
    /// The key for `model`'s VOXL with the current support layout. None when the model has no
    /// checksum to key it by.
    pub fn for_model(model: &ModelRow, raft_height_mm: f32) -> Option<Self> {
        (!model.checksum.is_empty()).then(|| VoxlCacheKey {
            checksum: model.checksum.clone(),
            file_name: model.file_name.clone(),
            raft_height_mm,
            supported: model.calculated_supported,
            layout_version: CURRENT_SUPPORT_LAYOUT_VERSION,
        })
    }

    fn file_name(&self) -> String {
        let canonical = format!(
            "{}|{}|{}|layout:{}|supported:{:?}",
            self.checksum,
            self.file_name,
            scan_config::compute(self.raft_height_mm),
            self.layout_version,
            self.supported
        );
        format!("{}.{ENTRY_EXTENSION}", hex::encode(&Sha256::digest(canonical.as_bytes())[..16]))
    }
}

#[derive(Clone)]
pub struct VoxlCache {
    inner: Arc<Inner>,
}

struct Inner {
    dir: PathBuf,
    max_bytes: u64,
    /// Held while evicting, so two writers don't both delete to make room.
    evicting: Mutex<()>,
}

impl VoxlCache {
    /// A cache in `dir` holding up to `max_bytes` of entries (0 = nothing is cached). Files
    /// left half-written by an earlier run are removed.
    pub fn new(dir: PathBuf, max_bytes: u64) -> Self {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if entry.path().extension().is_some_and(|e| e == PARTIAL_EXTENSION) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        VoxlCache { inner: Arc::new(Inner { dir, max_bytes, evicting: Mutex::new(()) }) }
    }

    /// The cached VOXL for `key`, marked as just read. Blocking.
    pub fn get(&self, key: &VoxlCacheKey) -> Option<Vec<u8>> {
        self.get_at(key, SystemTime::now())
    }

    /// Stores `bytes` as the VOXL for `key`, then evicts the entries read longest ago until
    /// the cache fits its limit. A VOXL bigger than the whole cache is not stored. Failures
    /// are logged, not returned: the download goes ahead uncached. Blocking.
    pub fn put(&self, key: &VoxlCacheKey, bytes: &[u8]) {
        self.put_at(key, bytes, SystemTime::now());
    }

    fn get_at(&self, key: &VoxlCacheKey, now: SystemTime) -> Option<Vec<u8>> {
        if self.inner.max_bytes == 0 {
            return None;
        }
        let path = self.inner.dir.join(key.file_name());
        // Evicted entries vanish between listing and reading; any failure is a miss.
        let bytes = fs::read(&path).ok()?;
        if let Err(err) = touch(&path, now) {
            warn!(target: LOG_TARGET, "Failed to mark {} as read: {err}", path.display());
        }
        Some(bytes)
    }

    fn put_at(&self, key: &VoxlCacheKey, bytes: &[u8], now: SystemTime) {
        if self.inner.max_bytes == 0 || bytes.len() as u64 > self.inner.max_bytes {
            return;
        }
        let path = self.inner.dir.join(key.file_name());
        if let Err(err) = self.write_entry(&path, bytes, now) {
            warn!(target: LOG_TARGET, "Failed to cache {}: {err}", path.display());
            return;
        }
        if let Err(err) = self.evict() {
            warn!(target: LOG_TARGET, "Failed to evict model VOXLs from {}: {err}", self.inner.dir.display());
        }
    }

    /// Writes beside the entry and renames it into place, so readers never see part of it.
    fn write_entry(&self, path: &Path, bytes: &[u8], now: SystemTime) -> std::io::Result<()> {
        fs::create_dir_all(&self.inner.dir)?;
        let partial = path.with_extension(format!("{}.{PARTIAL_EXTENSION}", Uuid::new_v4().simple()));
        let written = fs::write(&partial, bytes).and_then(|()| touch(&partial, now));
        if let Err(err) = written.and_then(|()| fs::rename(&partial, path)) {
            let _ = fs::remove_file(&partial);
            return Err(err);
        }
        Ok(())
    }

    /// Deletes the entries read longest ago until the rest fit the limit.
    fn evict(&self) -> std::io::Result<()> {
        let _evicting = self.inner.evicting.lock();
        let mut entries = Vec::new();
        for entry in fs::read_dir(&self.inner.dir)? {
            let entry = entry?;
            if entry.path().extension().is_none_or(|e| e != ENTRY_EXTENSION) {
                continue;
            }
            match entry.metadata() {
                Ok(meta) => entries.push((meta.modified()?, meta.len(), entry.path())),
                Err(err) if err.kind() == ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
        }
        let mut total: u64 = entries.iter().map(|(_, len, _)| len).sum();
        if total <= self.inner.max_bytes {
            return Ok(());
        }
        entries.sort_by_key(|(accessed, _, _)| *accessed);
        for (_, len, path) in entries {
            if total <= self.inner.max_bytes {
                break;
            }
            match fs::remove_file(&path) {
                Ok(()) => total -= len,
                Err(err) if err.kind() == ErrorKind::NotFound => total -= len,
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }
}

/// Sets `path`'s modified time, which records when the entry was last read.
fn touch(path: &Path, at: SystemTime) -> std::io::Result<()> {
    File::options().write(true).open(path)?.set_modified(at)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn key(checksum: &str) -> VoxlCacheKey {
        VoxlCacheKey {
            checksum: checksum.to_string(),
            file_name: "knight.stl".to_string(),
            raft_height_mm: 2.0,
            supported: Some(true),
            layout_version: CURRENT_SUPPORT_LAYOUT_VERSION,
        }
    }

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + seconds)
    }

    fn entry_count(dir: &Path) -> usize {
        fs::read_dir(dir).unwrap().count()
    }

    #[test]
    fn entries_are_read_back_by_their_key() {
        let dir = tempfile::tempdir().unwrap();
        let cache = VoxlCache::new(dir.path().join("voxl"), 1 << 20);
        assert_eq!(cache.get(&key("a")), None);
        cache.put(&key("a"), b"first");
        assert_eq!(cache.get(&key("a")).as_deref(), Some(&b"first"[..]));
        cache.put(&key("a"), b"second");
        assert_eq!(cache.get(&key("a")).as_deref(), Some(&b"second"[..]));
        assert_eq!(entry_count(&dir.path().join("voxl")), 1, "no partial files are left behind");
    }

    #[test]
    fn a_new_raft_height_layout_version_or_model_file_misses() {
        let dir = tempfile::tempdir().unwrap();
        let cache = VoxlCache::new(dir.path().to_path_buf(), 1 << 20);
        let cached = key("a");
        cache.put(&cached, b"voxl");
        let changes = [
            VoxlCacheKey { raft_height_mm: 1.5, ..cached.clone() },
            VoxlCacheKey { layout_version: cached.layout_version + 1, ..cached.clone() },
            VoxlCacheKey { checksum: "b".to_string(), ..cached.clone() },
            VoxlCacheKey { file_name: "renamed.stl".to_string(), ..cached.clone() },
            VoxlCacheKey { supported: None, ..cached.clone() },
        ];
        for changed in changes {
            assert_eq!(cache.get(&changed), None, "{changed:?}");
        }
        assert!(cache.get(&cached).is_some());
    }

    #[test]
    fn evicts_the_entries_read_longest_ago_when_full() {
        let dir = tempfile::tempdir().unwrap();
        let cache = VoxlCache::new(dir.path().to_path_buf(), 250);
        let entry = [0u8; 100];
        cache.put_at(&key("a"), &entry, at(0));
        cache.put_at(&key("b"), &entry, at(1));
        assert!(cache.get_at(&key("a"), at(2)).is_some());
        cache.put_at(&key("c"), &entry, at(3));
        assert!(cache.get_at(&key("b"), at(4)).is_none(), "b was read longest ago");
        assert!(cache.get_at(&key("c"), at(5)).is_some());
        assert!(cache.get_at(&key("a"), at(6)).is_some());

        // c is now the one read longest ago; d replaces it.
        cache.put_at(&key("d"), &entry, at(7));
        assert!(cache.get_at(&key("c"), at(8)).is_none());
        assert!(cache.get_at(&key("a"), at(9)).is_some());
        assert!(cache.get_at(&key("d"), at(10)).is_some());
        assert_eq!(entry_count(dir.path()), 2);
    }

    #[test]
    fn a_voxl_bigger_than_the_cache_or_a_zero_size_cache_stores_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let cache = VoxlCache::new(dir.path().join("small"), 10);
        cache.put(&key("a"), &[0u8; 11]);
        assert_eq!(cache.get(&key("a")), None);

        let disabled = VoxlCache::new(dir.path().join("off"), 0);
        disabled.put(&key("a"), b"");
        assert_eq!(disabled.get(&key("a")), None);
        assert!(!dir.path().join("off").exists());
    }

    #[test]
    fn half_written_entries_from_an_earlier_run_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(format!("abc.123.{PARTIAL_EXTENSION}")), b"half").unwrap();
        fs::write(dir.path().join(format!("abc.{ENTRY_EXTENSION}")), b"whole").unwrap();
        VoxlCache::new(dir.path().to_path_buf(), 1 << 20);
        assert_eq!(entry_count(dir.path()), 1);
    }
}
