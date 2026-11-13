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
pub const CURRENT_REPAIR_VERSION: i32 = 3;

/// The repair's remesh grid (mm) and the simplification bound of its remeshed patches.
pub const REPAIR_CELL_MM: f64 = 0.05;
pub const REPAIR_TOLERANCE_MM: f64 = 0.01;

/// A model mesh made watertight for printing ([`union::hybrid`], then [`mesh::finish`]): closed,
/// manifold and consistently oriented, the exact surface kept outside the mended spots, sealed
/// voids filled and drained cavities kept. Supports are never passed in.
///
/// The repair runs on a pool of `threads` threads of its own ([`repair_pool`]). How its work is
/// split, and so the exact output, depends on the count, but every count gives a watertight and
/// valid mesh, so the count is not part of the key: a repair made on other threads is used as it is.
///
/// A mesh is repaired once: a repair kept in `store` is used when there is one, a call for a mesh
/// (by its corners) already being repaired, by a model download or a plate
/// export, waits for that repair and gets its result, and a new repair is kept in `store`. Call
/// it from a thread outside rayon's pools (a blocking task, or a thread of its own): waiting
/// happens on the calling thread, and a pool thread kept waiting could hold work the running
/// repair needs.
pub fn repair_model(triangles: &[Triangle], store: Option<&dyn RepairStore>, threads: usize) -> Repair {
    let threads = threads.clamp(1, MAX_REPAIR_THREADS);
    let key = mesh_key(triangles);
    if let Some(stored) = store.and_then(|s| s.load(&key)) {
        return Repair { triangles: stored, key };
    }
    let shared = repair_shared(triangles, key, threads, store);
    // The only holder once the waiters have taken theirs: no copy then.
    let triangles = std::sync::Arc::try_unwrap(shared).unwrap_or_else(|shared| shared.as_ref().clone());
    Repair { triangles, key }
}

/// Which faces of `repaired` (a repair of `original`) the repair made: those whose corners are not
/// the corners of one of the original's faces (by exact position, in any order or winding). The
/// repair keeps the exact surface outside the mended spots, so the rest match bit for bit.
pub fn changed_faces(original: &[Triangle], repaired: &[Triangle]) -> Vec<bool> {
    use rayon::prelude::*;
    // `+ 0.0` makes -0 and +0 one corner.
    let key = |t: &Triangle| {
        let mut corners = [t.v0, t.v1, t.v2].map(|v| [v.x + 0.0, v.y + 0.0, v.z + 0.0].map(f32::to_bits));
        corners.sort_unstable();
        corners
    };
    let faces: rustc_hash::FxHashSet<[[u32; 3]; 3]> = original.par_iter().map(key).collect();
    repaired.par_iter().map(|t| !faces.contains(&key(t))).collect()
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

/// A repaired mesh and its key ([`mesh_key`]).
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
    threads: usize,
    store: Option<&dyn RepairStore>,
) -> std::sync::Arc<Vec<Triangle>> {
    REPAIRS.run(key, || {
        // Kept by a repair that finished after the caller looked.
        if let Some(stored) = store.and_then(|s| s.load(&key)) {
            return stored;
        }
        let repaired = match repair_pool(threads) {
            Some(pool) => pool.install(|| {
                #[cfg(test)]
                tests::REPAIR_THREADS.lock().insert(key, rayon::current_num_threads());
                repair_now(triangles)
            }),
            None => repair_now(triangles),
        };
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

/// The most threads a repair runs on.
pub const MAX_REPAIR_THREADS: usize = 256;
/// The threads a repair runs on until the settings say otherwise.
pub const DEFAULT_REPAIR_THREADS: usize = 20;

/// The repairs' thread pool, of `threads` threads: built once and kept while repairs ask for that
/// many, rebuilt when they ask for another count (a repair already running keeps the pool it
/// started on). None when the pool cannot be built (the repair then runs on rayon's global pool).
pub fn repair_pool(threads: usize) -> Option<std::sync::Arc<rayon::ThreadPool>> {
    static POOL: PoolCache = PoolCache::new();
    POOL.get(threads)
}

/// A thread pool kept for one thread count at a time ([`repair_pool`]).
struct PoolCache(parking_lot::Mutex<Option<std::sync::Arc<rayon::ThreadPool>>>);

impl PoolCache {
    const fn new() -> Self {
        PoolCache(parking_lot::Mutex::new(None))
    }

    fn get(&self, threads: usize) -> Option<std::sync::Arc<rayon::ThreadPool>> {
        let mut pool = self.0.lock();
        if let Some(p) = pool.as_ref().filter(|p| p.current_num_threads() == threads) {
            return Some(p.clone());
        }
        match rayon::ThreadPoolBuilder::new().num_threads(threads).thread_name(|i| format!("mesh-repair-{i}")).build() {
            Ok(built) => {
                let built = std::sync::Arc::new(built);
                *pool = Some(built.clone());
                Some(built)
            }
            Err(err) => {
                tracing::warn!("Failed to build the mesh repair's pool of {threads} threads: {err}");
                None
            }
        }
    }
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

    /// The thread count each repair in the tests ran with, by key.
    pub(super) static REPAIR_THREADS: std::sync::LazyLock<
        parking_lot::Mutex<std::collections::HashMap<MeshKey, usize>>,
    > = std::sync::LazyLock::new(Default::default);

    #[test]
    fn a_repair_runs_on_a_pool_of_the_threads_asked_for() {
        let original = sphere(6.0, 24);
        for threads in [1, 3] {
            let repair = repair_model(&original, None, threads);
            assert_eq!(repair.key, mesh_key(&original), "the count is not part of the key");
            assert_eq!(REPAIR_THREADS.lock().get(&repair.key), Some(&threads));
        }
        repair_model(&original, None, 0);
        assert_eq!(REPAIR_THREADS.lock().get(&mesh_key(&original)), Some(&1), "at least one");
    }

    #[test]
    fn changed_faces_are_those_not_in_the_original_by_their_corners() {
        let (a, b, c, d) =
            (Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, -0.0));
        let original = [Triangle::new(a, b, c, Vec3::UP), Triangle::new(a, c, Vec3::new(0.0, 0.0, 1.0), Vec3::UP)];
        let repaired = [
            // Its corners in another order, and wound the other way: the same face.
            Triangle::new(b, c, d, Vec3::default()),
            Triangle::new(Vec3::new(0.0, 0.0, 1.0), c, a, Vec3::default()),
            // Moved by one bit: made by the repair.
            Triangle::new(a, b, Vec3::new(0.0, f32::from_bits(1.0f32.to_bits() + 1), 0.0), Vec3::default()),
        ];
        assert_eq!(changed_faces(&original, &repaired), vec![false, false, true]);
    }

    #[test]
    fn a_closed_mesh_repairs_with_no_changed_faces_and_a_crossing_one_with_some() {
        let ball = sphere(6.5, 24);
        let repaired = repair_model(&ball, None, 2).triangles;
        assert!(changed_faces(&ball, &repaired).iter().all(|c| !c));
        // Two balls crossing: the union cuts both along the crossing, and keeps the rest.
        let mut crossing = sphere(5.5, 24);
        crossing.extend(sphere(5.5, 24).into_iter().map(|t| {
            let m = |v: Vec3| Vec3::new(v.x + 4.03, v.y + 0.01, v.z + 0.02);
            Triangle::new(m(t.v0), m(t.v1), m(t.v2), t.normal)
        }));
        let repaired = repair_model(&crossing, None, 2).triangles;
        let changed = changed_faces(&crossing, &repaired);
        let count = changed.iter().filter(|&&c| c).count();
        assert!(count > 0 && count < changed.len() / 2, "{count} of {}", changed.len());
    }

    #[test]
    fn the_pool_is_kept_for_one_count_and_rebuilt_for_another() {
        let cache = PoolCache::new();
        let a = cache.get(3).unwrap();
        assert!(std::sync::Arc::ptr_eq(&a, &cache.get(3).unwrap()), "kept");
        assert_eq!(a.install(rayon::current_num_threads), 3);
        let b = cache.get(5).unwrap();
        assert_eq!(b.install(rayon::current_num_threads), 5);
        assert!(!std::sync::Arc::ptr_eq(&a, &cache.get(3).unwrap()), "rebuilt for the new count");
        // A pool handed out before the change still works for the repair holding it.
        assert_eq!(b.install(rayon::current_num_threads), 5);
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
                repair_shared(&original, key, 2, None)
            });
            let b = s.spawn(|| {
                gate.wait();
                repair_shared(&original, key, 2, None)
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
        let first = repair_model(&original, Some(&store), 2);
        assert_eq!(first.key, mesh_key(&original));
        assert_eq!(store.saves.load(Ordering::SeqCst), 1, "a new repair is kept");
        let second = repair_model(&original, Some(&store), 2);
        assert_eq!(store.saves.load(Ordering::SeqCst), 1, "the kept one is used");
        assert_eq!(second.triangles, first.triangles);
        // Another mesh is repaired for itself.
        let other = sphere(8.5, 30);
        assert_ne!(repair_model(&other, Some(&store), 2).key, first.key);
        assert_eq!(store.saves.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_repaired_mesh_is_written_about_as_compactly_as_the_original() {
        let original = sphere(10.0, 60);
        let repaired = repair_model(&original, None, 4).triangles;
        assert_eq!(repaired.len(), original.len(), "a closed sphere comes through as it is");
        assert!(repaired.iter().all(|t| t.normal == Vec3::default()), "no stored normals");
        let (before, after) = (voxl_size(&original), voxl_size(&repaired));
        assert!(after * 10 < before * 11, "{after} bytes against {before}");
    }
}
