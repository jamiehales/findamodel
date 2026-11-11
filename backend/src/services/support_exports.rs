//! Each pre-supported model's export entry: which of its triangles are supports and the supports
//! read from them ([`ModelExport`]), so model and plate VOXL exports don't read the supports again.
//!
//! Entries are files (like previews), named by a hash of their key: the model file's checksum, its
//! scan config (raft height and detection version), its supported metadata and whether its
//! detection was checked. The file is the authority: its header holds the reader and detection
//! versions and the key, and an entry is used only when all of them are current. The `models` row
//! keeps an index of the entry (`support_export_path`, `support_export_version`) for the scan to
//! find stale ones; when it disagrees with the file, the file wins. Entries are kept apart from the
//! VOXL download cache and are never evicted; the scan writes them and exports rewrite any that
//! are missing or out of date.

pub mod codec;

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tracing::warn;
use uuid::Uuid;

use crate::entities::ModelRow;
use crate::geometry::Triangle;
use crate::geometry::support_layout::SupportLayout;
use crate::geometry::support_reader::CURRENT_SUPPORT_READER_VERSION;
use crate::geometry::support_separation::{self, CURRENT_SUPPORT_DETECTION_VERSION};
use crate::services::scan_config;
pub use codec::{Header, ModelExport};

const LOG_TARGET: &str = "findamodel.SupportExports";
const EXTENSION: &str = "export";
/// Enough of a file to hold any header (the key is a few hundred bytes).
const HEADER_READ_BYTES: u64 = 4096;

/// What an entry is made from.
pub fn key(checksum: &str, raft_height_mm: f32, supported: Option<bool>, checked: bool) -> String {
    format!("{checksum}|{}|supported:{supported:?}|checked:{checked}", scan_config::compute(raft_height_mm))
}

/// The key of `model`'s entry, from its stored checksum and supported metadata.
pub fn key_for(model: &ModelRow, raft_height_mm: f32, checked: bool) -> String {
    key(&model.checksum, raft_height_mm, model.calculated_supported, checked)
}

/// The entry file's path under the store, relative.
pub fn relative_path(key: &str) -> String {
    let hash = hex::encode(&Sha256::digest(key.as_bytes())[..16]);
    format!("{}/{hash}.{EXTENSION}", &hash[..2])
}

/// The header an entry for `key` is written with now.
fn current_header(key: &str) -> Header {
    Header {
        reader_version: CURRENT_SUPPORT_READER_VERSION,
        detection_version: CURRENT_SUPPORT_DETECTION_VERSION,
        key: key.to_string(),
    }
}

/// An export read from an entry: the model's body (Z-up) and rebuilt supports, or None when it
/// holds nothing to rebuild.
pub type Export = Option<(Vec<Triangle>, SupportLayout)>;

#[derive(Clone)]
pub struct SupportExports {
    dir: Arc<PathBuf>,
}

impl SupportExports {
    pub fn new(dir: PathBuf) -> Self {
        SupportExports { dir: Arc::new(dir) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The export stored for `key`, when its entry is whole and at the current reader and
    /// detection versions, with the model's body split off by its mask. `triangles` are the
    /// model's loaded (Y-up) triangles; an entry for another triangle count is not used. Blocking.
    pub fn read(&self, key: &str, triangles: &[Triangle]) -> Option<Export> {
        let bytes = fs::read(self.dir.join(relative_path(key))).ok()?;
        let (header, export) = codec::decode(&bytes)?;
        if header != current_header(key) {
            return None;
        }
        match export {
            None => Some(None),
            Some((mask, layout)) if mask.len() == triangles.len() => {
                let (body, _) = support_separation::split(triangles, &mask);
                Some(Some((crate::geometry::to_z_up(&body), layout)))
            }
            Some(_) => None,
        }
    }

    /// The reader version of the entry stored for `key`, read from its file: None when there is
    /// none, it is damaged, it is for another key or another detection version. Blocking.
    pub fn stored_version(&self, key: &str) -> Option<i32> {
        let mut head = Vec::new();
        fs::File::open(self.dir.join(relative_path(key))).ok()?.take(HEADER_READ_BYTES).read_to_end(&mut head).ok()?;
        let header = codec::decode_header(&head)?;
        (header.key == key && header.detection_version == CURRENT_SUPPORT_DETECTION_VERSION)
            .then_some(header.reader_version)
    }

    /// Stores the export for `key` at the current versions, replacing any entry there. Returns
    /// the entry's relative path. Blocking.
    pub fn write(&self, key: &str, export: Option<(&[bool], &SupportLayout)>) -> std::io::Result<String> {
        let relative = relative_path(key);
        let path = self.dir.join(&relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Written beside the entry and renamed into place, so readers never see part of it.
        let partial = path.with_extension(format!("{}.partial", Uuid::new_v4().simple()));
        let written = fs::write(&partial, codec::encode(&current_header(key), export));
        if let Err(err) = written.and_then(|()| fs::rename(&partial, &path)) {
            let _ = fs::remove_file(&partial);
            return Err(err);
        }
        Ok(relative)
    }

    /// [`SupportExports::write`], logging a failure instead of returning it: the export goes
    /// ahead without its entry.
    pub fn store(&self, key: &str, export: Option<(&[bool], &SupportLayout)>) -> Option<String> {
        match self.write(key, export) {
            Ok(relative) => Some(relative),
            Err(err) => {
                warn!(target: LOG_TARGET, "Failed to store support export {}: {err}", relative_path(key));
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Vec3;

    fn triangles(n: usize) -> Vec<Triangle> {
        (0..n)
            .map(|i| {
                let z = i as f32;
                Triangle::new(
                    Vec3::new(0.0, z, 0.0),
                    Vec3::new(1.0, z, 0.0),
                    Vec3::new(0.0, z, 1.0),
                    Vec3::new(0.0, 1.0, 0.0),
                )
            })
            .collect()
    }

    #[test]
    fn an_entry_is_read_back_with_its_body_split_off() {
        let dir = tempfile::tempdir().unwrap();
        let store = SupportExports::new(dir.path().to_path_buf());
        let k = key("abc", 2.0, Some(true), true);
        let tris = triangles(4);
        assert_eq!(store.read(&k, &tris), None);
        assert_eq!(store.stored_version(&k), None);
        let layout = SupportLayout::default();
        let relative = store.write(&k, Some((&[true, false, true, false], &layout))).unwrap();
        assert_eq!(relative, relative_path(&k));
        assert_eq!(store.read(&k, &tris), Some(Some((crate::geometry::to_z_up(&[tris[1], tris[3]]), layout))));
        assert_eq!(store.stored_version(&k), Some(CURRENT_SUPPORT_READER_VERSION));
        // Another mesh, or another key, does not use it.
        assert_eq!(store.read(&k, &tris[..3]), None);
        assert_eq!(store.read(&key("abc", 2.0, Some(true), false), &tris), None);
        store.write(&k, None).unwrap();
        assert_eq!(store.read(&k, &tris), Some(None));
        assert_eq!(fs::read_dir(dir.path().join(&relative[..2])).unwrap().count(), 1, "no partial files left");
    }

    #[test]
    fn an_entry_at_another_version_or_damaged_is_not_used() {
        let dir = tempfile::tempdir().unwrap();
        let store = SupportExports::new(dir.path().to_path_buf());
        let k = key("abc", 2.0, Some(true), true);
        let tris = triangles(2);
        let path = dir.path().join(relative_path(&k));
        let write = |header: Header| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, codec::encode(&header, Some((&[true, false], &SupportLayout::default())))).unwrap();
        };
        write(Header { reader_version: CURRENT_SUPPORT_READER_VERSION - 1, ..current_header(&k) });
        assert_eq!(store.read(&k, &tris), None);
        assert_eq!(store.stored_version(&k), Some(CURRENT_SUPPORT_READER_VERSION - 1));
        write(Header { detection_version: CURRENT_SUPPORT_DETECTION_VERSION - 1, ..current_header(&k) });
        assert_eq!(store.read(&k, &tris), None);
        assert_eq!(store.stored_version(&k), None);
        write(Header { key: "another".into(), ..current_header(&k) });
        assert_eq!(store.read(&k, &tris), None);
        assert_eq!(store.stored_version(&k), None);
        write(current_header(&k));
        assert!(store.read(&k, &tris).is_some());
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, &bytes[..bytes.len() - 3]).unwrap();
        assert_eq!(store.read(&k, &tris), None, "a cut entry is not used");
        fs::write(&path, b"not an entry").unwrap();
        assert_eq!(store.read(&k, &tris), None);
        assert_eq!(store.stored_version(&k), None);
    }

    #[test]
    fn keys_tell_models_and_their_inputs_apart() {
        let base = key("abc", 2.0, Some(true), true);
        for other in [
            key("abd", 2.0, Some(true), true),
            key("abc", 1.5, Some(true), true),
            key("abc", 2.0, None, true),
            key("abc", 2.0, Some(true), false),
        ] {
            assert_ne!(relative_path(&other), relative_path(&base));
        }
    }
}
