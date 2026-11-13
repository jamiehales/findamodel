//! Watertight export meshes (the remeshed VOXL download and plate export): checks that say whether a
//! mesh is closed, manifold, consistently oriented and free of self-intersections, the shells'
//! nesting (sealed voids, inner skins facing the wrong way, buried parts), and two repairs: the
//! boolean self-union ([`union`]) that keeps the exact surface, and the level-set remesh
//! ([`remesh`]) that rebuilds it on a fine grid. [`measure`] compares a repair with its original.

pub mod intersect;
pub mod measure;
pub mod mesh;
pub mod remesh;
pub mod shells;
pub mod simplify;
pub mod union;

use crate::geometry::Triangle;
use crate::single_flight::SingleFlight;

/// Increment when the repair's output changes, so cached repaired exports are rebuilt.
pub const CURRENT_REPAIR_VERSION: i32 = 2;

/// The repair's remesh grid (mm) and the simplification bound of its remeshed patches.
pub const REPAIR_CELL_MM: f64 = 0.05;
pub const REPAIR_TOLERANCE_MM: f64 = 0.01;

/// A model mesh made watertight for printing ([`union::hybrid`], then [`mesh::finish`]): closed,
/// manifold and consistently oriented, the exact surface kept outside the mended spots, sealed
/// voids filled and drained cavities kept. Supports are never passed in.
///
/// A mesh is repaired once: a repair kept in `store` is used when there is one, a call for a mesh
/// (by its corners) already being repaired, by a model download or a plate export, waits for that
/// repair and gets its result, and a new repair is kept in `store`. Call it from a thread outside
/// rayon's pool (a blocking task, or a thread of its own): a pool thread kept waiting could hold
/// work the running repair needs.
pub fn repair_model(triangles: &[Triangle], store: Option<&dyn RepairStore>) -> Repair {
    let key = mesh_key(triangles);
    if let Some(stored) = store.and_then(|s| s.load(&key)) {
        return Repair { triangles: stored, key };
    }
    let shared = repair_shared(triangles, key, store);
    // The only holder once the waiters have taken theirs: no copy then.
    let triangles = std::sync::Arc::try_unwrap(shared).unwrap_or_else(|shared| shared.as_ref().clone());
    Repair { triangles, key }
}

/// A mesh's content key ([`mesh_key`]): a SHA-256 of its corners.
pub type MeshKey = [u8; 32];

/// Where repaired meshes are kept between repairs, by the key of the mesh each was repaired from
/// (at [`CURRENT_REPAIR_VERSION`]). Both calls are blocking.
pub trait RepairStore: Sync {
    /// The repaired mesh kept for `key`, when there is a whole one.
    fn load(&self, key: &MeshKey) -> Option<Vec<Triangle>>;
    /// Keeps `repaired` as the repair of the mesh with `key`; a failure is the store's to log.
    fn save(&self, key: &MeshKey, repaired: &[Triangle]);
}

/// A repaired mesh and the key of the mesh it was repaired from.
pub struct Repair {
    pub triangles: Vec<Triangle>,
    pub key: MeshKey,
}

/// How many [`repair_model`] calls so far got the result of a repair another call ran.
pub fn repairs_shared() -> usize {
    REPAIRS.shared()
}

/// The repair of the mesh with `key`, shared with the other calls that waited for it.
fn repair_shared(
    triangles: &[Triangle],
    key: MeshKey,
    store: Option<&dyn RepairStore>,
) -> std::sync::Arc<Vec<Triangle>> {
    REPAIRS.run(key, || {
        // Kept by a repair that finished after the caller looked.
        if let Some(stored) = store.and_then(|s| s.load(&key)) {
            return stored;
        }
        let repaired = repair_now(triangles);
        if let Some(s) = store {
            s.save(&key, &repaired);
        }
        repaired
    })
}

/// The repairs under way, by [`mesh_key`].
static REPAIRS: std::sync::LazyLock<SingleFlight<MeshKey, Vec<Triangle>>> =
    std::sync::LazyLock::new(SingleFlight::default);

/// A hash of a mesh's corners (normals play no part in the repair), in chunks hashed in parallel.
pub fn mesh_key(triangles: &[Triangle]) -> MeshKey {
    use rayon::prelude::*;
    use sha2::{Digest, Sha256};
    let chunks: Vec<[u8; 32]> = triangles
        .par_chunks(1 << 16)
        .map(|c| {
            let mut h = Sha256::new();
            for t in c {
                for v in [t.v0, t.v1, t.v2] {
                    for x in [v.x, v.y, v.z] {
                        h.update(x.to_bits().to_le_bytes());
                    }
                }
            }
            h.finalize().into()
        })
        .collect();
    let mut h = Sha256::new();
    h.update((triangles.len() as u64).to_le_bytes());
    for c in &chunks {
        h.update(c);
    }
    h.finalize().into()
}

/// [`repair_model`] without waiting for or sharing with other calls.
fn repair_now(triangles: &[Triangle]) -> Vec<Triangle> {
    let pts = crate::geometry::support_reader::math::points_of(triangles);
    let (repaired, _) = union::hybrid(&pts, REPAIR_CELL_MM, REPAIR_TOLERANCE_MM);
    let (finished, _) = mesh::finish(&repaired);
    in_original_order(triangles, &finished)
}

/// The repaired faces as triangles for writing: faces kept from the original come first, in the
/// original's order and starting from its first corner, then the new ones. A boolean returns its
/// faces sorted along a space-filling curve, which scatters a mesh written row by row and
/// compresses far worse. No normal is stored (readers take it from the winding, as STL allows):
/// a computed normal is 12 bytes of noise per face.
fn in_original_order(original: &[Triangle], m: &mesh::Indexed) -> Vec<Triangle> {
    use crate::geometry::Vec3;
    let key = |v: Vec3| [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()];
    let mut by_corners: rustc_hash::FxHashMap<[[u32; 3]; 3], usize> = rustc_hash::FxHashMap::default();
    for (i, t) in original.iter().enumerate() {
        let mut c = [key(t.v0), key(t.v1), key(t.v2)];
        c.sort_unstable();
        by_corners.entry(c).or_insert(i);
    }
    let mut faces: Vec<(usize, usize, [Vec3; 3])> = m
        .faces
        .iter()
        .enumerate()
        .map(|(k, f)| {
            let v = f.map(|i| {
                let p = m.verts[i as usize];
                Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
            });
            let mut c = v.map(key);
            c.sort_unstable();
            match by_corners.get(&c) {
                Some(&i) => {
                    // The original's corner order (the same winding, so a rotation).
                    let first = key(original[i].v0);
                    let r = (0..3).find(|&r| key(v[r]) == first).unwrap_or(0);
                    (i, k, [v[r], v[(r + 1) % 3], v[(r + 2) % 3]])
                }
                None => (usize::MAX, k, v),
            }
        })
        .collect();
    faces.sort_unstable_by_key(|f| (f.0, f.1));
    faces.into_iter().map(|(_, _, v)| Triangle::new(v[0], v[1], v[2], Vec3::default())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Vec3;
    use crate::geometry::voxl::{self, VoxlMesh, VoxlPlacement};

    /// A finely tessellated sphere stored with no normals, as many STL files are.
    fn sphere(r: f32, n: usize) -> Vec<Triangle> {
        let at = |i: usize, j: usize| {
            if i == 0 || i == n {
                return Vec3::new(0.0, 0.0, if i == 0 { r } else { -r });
            }
            let (t, p) =
                (std::f32::consts::PI * i as f32 / n as f32, std::f32::consts::TAU * (j % n) as f32 / n as f32);
            Vec3::new(r * t.sin() * p.cos(), r * t.sin() * p.sin(), r * t.cos())
        };
        let mut out = Vec::new();
        for i in 0..n {
            for j in 0..n {
                let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
                if i > 0 {
                    out.push(Triangle::new(a, b, d, Vec3::default()));
                }
                if i + 1 < n {
                    out.push(Triangle::new(b, c, d, Vec3::default()));
                }
            }
        }
        out
    }

    fn voxl_size(triangles: &[Triangle]) -> usize {
        let mesh = VoxlMesh { name: "ball.stl", triangles, supports: None };
        let placement = VoxlPlacement { mesh: 0, rotation_z: 0.0, translation: [0.0; 3] };
        voxl::write_voxl(std::io::Cursor::new(Vec::new()), &[mesh], &[placement], None).unwrap().into_inner().len()
    }

    /// A store in memory, counting what it keeps.
    #[derive(Default)]
    struct Memory {
        kept: parking_lot::Mutex<std::collections::HashMap<MeshKey, Vec<Triangle>>>,
        saves: std::sync::atomic::AtomicUsize,
    }

    impl RepairStore for Memory {
        fn load(&self, key: &MeshKey) -> Option<Vec<Triangle>> {
            self.kept.lock().get(key).cloned()
        }
        fn save(&self, key: &MeshKey, repaired: &[Triangle]) {
            self.saves.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.kept.lock().insert(*key, repaired.to_vec());
        }
    }

    #[test]
    fn two_repairs_of_one_mesh_at_once_share_one_result() {
        let original = sphere(7.0, 40);
        let key = mesh_key(&original);
        let gate = std::sync::Barrier::new(2);
        let (a, b) = std::thread::scope(|s| {
            let a = s.spawn(|| {
                gate.wait();
                repair_shared(&original, key, None)
            });
            let b = s.spawn(|| {
                gate.wait();
                repair_shared(&original, key, None)
            });
            (a.join().unwrap(), b.join().unwrap())
        });
        assert!(std::sync::Arc::ptr_eq(&a, &b), "both callers get the one repair");
        assert_eq!(a.len(), original.len());
    }

    #[test]
    fn a_kept_repair_is_used_instead_of_repairing_again() {
        use std::sync::atomic::Ordering;
        let original = sphere(8.0, 30);
        let store = Memory::default();
        let first = repair_model(&original, Some(&store));
        assert_eq!(first.key, mesh_key(&original));
        assert_eq!(store.saves.load(Ordering::SeqCst), 1, "a new repair is kept");
        let second = repair_model(&original, Some(&store));
        assert_eq!(store.saves.load(Ordering::SeqCst), 1, "the kept one is used");
        assert_eq!(second.triangles, first.triangles);
        // Another mesh is repaired for itself.
        let other = sphere(8.5, 30);
        assert_ne!(repair_model(&other, Some(&store)).key, first.key);
        assert_eq!(store.saves.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_repaired_mesh_is_written_about_as_compactly_as_the_original() {
        let original = sphere(10.0, 60);
        let repaired = repair_model(&original, None).triangles;
        assert_eq!(repaired.len(), original.len(), "a closed sphere comes through as it is");
        assert!(repaired.iter().all(|t| t.normal == Vec3::default()), "no stored normals");
        let (before, after) = (voxl_size(&original), voxl_size(&repaired));
        assert!(after * 10 < before * 11, "{after} bytes against {before}");
    }
}
