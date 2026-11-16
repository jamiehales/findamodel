//! The remeshed twin lookup in the app ([`crate::geometry::remesh_twin`]): the twin lookup's
//! fallback ([`crate::services::twins`]) when no exact twin splits a supported model.
//!
//! - **Descriptors.** The scan writes each model's descriptor (`model_remesh_descriptors`) from the
//!   geometry it has loaded, with the twin sketch, so it follows the scan config
//!   ([`crate::services::scan_config`] holds the remeshed twin version).
//! - **Candidates** come from stored descriptors only: models not marked supported with the same
//!   creator, collection and subcollection as the supported model ([`candidates`]). A model
//!   without a creator or collection is not looked up. Only the few candidates verified are loaded.
//! - **Record.** What the lookup measured goes in the model's `twin_lookup` row ([`detail`]); a
//!   twin found makes its method `twin_remeshed`, and the split's `support_split` row
//!   ([`crate::services::processing::SplitMethod::TwinRemeshed`]).

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::json;
use uuid::Uuid;

use crate::db::{Db, now};
use crate::entities::ModelRow;
use crate::geometry::Triangle;
use crate::geometry::loader;
use crate::geometry::remesh_twin::{self, CURRENT_REMESH_TWIN_VERSION, Descriptor, Lookup};
use crate::geometry::twin::Mesh;
use crate::metadata::inheritance;

/// The stored descriptor of loaded triangles (as a candidate twin: nothing cut). Blocking.
pub fn descriptor_bytes(triangles: &[Triangle]) -> Vec<u8> {
    Descriptor::of_triangles(triangles, false).to_bytes()
}

/// Stores a model's descriptor; `described_at` moves only when it changed, so supported models
/// looked up before are looked up again against it only then.
pub async fn store_descriptor(db: &Db, model_id: Uuid, descriptor: &[u8]) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO model_remesh_descriptors (model_id, version, descriptor, described_at) VALUES (?, ?, ?, ?)
         ON CONFLICT(model_id) DO UPDATE SET version = excluded.version, descriptor = excluded.descriptor,
            described_at = excluded.described_at
         WHERE model_remesh_descriptors.descriptor IS NOT excluded.descriptor
            OR model_remesh_descriptors.version != excluded.version",
    )
    .bind(model_id)
    .bind(CURRENT_REMESH_TWIN_VERSION)
    .bind(descriptor)
    .bind(now())
    .execute(db)
    .await?;
    Ok(())
}

/// A candidate twin with its stored descriptor.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: Uuid,
    pub checksum: String,
    pub path: PathBuf,
    pub file_type: String,
    pub descriptor: Descriptor,
}

#[derive(sqlx::FromRow)]
struct CandidateRow {
    model_id: Uuid,
    descriptor: Vec<u8>,
    checksum: String,
    directory: String,
    file_name: String,
    file_type: String,
}

/// The candidate twins of supported `model`: models not marked supported (unmarked included)
/// with its creator, collection and subcollection and a current descriptor, other than itself
/// and its copies; only those described after `since` when given. None without a creator or
/// collection.
pub async fn candidates(
    db: &Db,
    model: &ModelRow,
    root: &Path,
    since: Option<DateTime<Utc>>,
) -> sqlx::Result<Vec<Candidate>> {
    let (Some(creator), Some(collection)) = (&model.calculated_creator, &model.calculated_collection) else {
        return Ok(Vec::new());
    };
    let rows: Vec<CandidateRow> = sqlx::query_as(
        "SELECT d.model_id, d.descriptor, m.checksum, m.directory, m.file_name, m.file_type
         FROM models m JOIN model_remesh_descriptors d ON d.model_id = m.id
         WHERE m.calculated_creator = ?1 AND m.calculated_collection = ?2 AND m.calculated_subcollection IS ?3
           AND m.effective_supported IS NOT 1 AND d.version = ?4 AND m.id != ?5 AND m.checksum != ?6
           AND (?7 IS NULL OR d.described_at > ?7)
         ORDER BY m.id",
    )
    .bind(creator)
    .bind(collection)
    .bind(&model.calculated_subcollection)
    .bind(CURRENT_REMESH_TWIN_VERSION)
    .bind(model.id)
    .bind(&model.checksum)
    .bind(since)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            Some(Candidate {
                descriptor: Descriptor::from_bytes(&r.descriptor)?,
                id: r.model_id,
                checksum: r.checksum,
                path: inheritance::build_full_file_path(root, &r.directory, &r.file_name),
                file_type: r.file_type,
            })
        })
        .collect())
}

/// Looks up supported `s` (its mesh) among `candidates`, loading only those verified. Blocking.
pub fn look_up(s: Mesh, candidates: &[Candidate]) -> Lookup {
    let descriptors: Vec<Descriptor> = candidates.iter().map(|c| c.descriptor.clone()).collect();
    remesh_twin::lookup_mesh(s, &descriptors, |i| {
        loader::load_model(&candidates[i].path, &candidates[i].file_type).map(|g| g.triangles)
    })
}

/// The chosen twin of a lookup: its candidate, scale and coverage.
pub fn chosen<'a>(lookup: &Lookup, candidates: &'a [Candidate]) -> Option<(&'a Candidate, f64, f64)> {
    let c = lookup.chosen_check()?;
    let scale = c.check.transform.map_or(1.0, |x| x.scale);
    Some((&candidates[c.ranked.index], scale, c.check.coverage))
}

/// What a remeshed lookup measured, for the `twin_lookup` row's detail.
pub fn detail(lookup: Option<&Lookup>, candidates: &[Candidate], ms: f64) -> serde_json::Value {
    let Some(lookup) = lookup else {
        return json!({ "candidates": candidates.len(), "ms": ms });
    };
    let ranked: Vec<serde_json::Value> = lookup
        .ranked
        .iter()
        .take(remesh_twin::MAX_CANDIDATES + 2)
        .map(|r| {
            json!({
                "twinId": candidates[r.index].id,
                "containment": r.containment,
                "areaRatio": r.area_ratio,
                "spreadRatio": r.spread_ratio,
                "plausible": r.plausible(),
            })
        })
        .collect();
    let verified: Vec<serde_json::Value> = lookup
        .checked
        .iter()
        .map(|c| {
            let k = &c.check;
            json!({
                "twinId": candidates[c.ranked.index].id,
                "verdict": k.verdict.name(),
                "scale": k.transform.map(|x| x.scale),
                "poses": k.poses,
                "inliers": k.inliers,
                "deviationMm": { "p50": k.deviation.p50, "p90": k.deviation.p90, "p99": k.deviation.p99 },
                "coverage": k.coverage,
                "largestGap": k.largest_gap,
                "patchMm2": k.patch_mm2,
                "thickSupportMm2": k.thick_support_mm2,
                "thickSupportShare": k.thick_support_share,
                "thickSupportFlagged": k.thick_flagged,
                "explainedMm2": k.explained_mm2,
                "twinAreaMm2": k.twin_area,
                "supportShare": k.shape.support_share,
                "seamLoops": k.shape.seam_loops,
                "longestSeamMm": k.shape.longest_seam_mm,
                "alignMs": k.align_ms,
                "labelMs": k.label_ms,
            })
        })
        .collect();
    let split = lookup.split.as_ref().map(|x| {
        json!({
            "innerWall": x.inner,
            "cleaned": [x.cleaned.0, x.cleaned.1],
            "supportLike": x.supports,
        })
    });
    json!({
        "candidates": candidates.len(),
        "plausible": lookup.ranked.iter().filter(|r| r.plausible()).count(),
        "ranked": ranked,
        "verified": verified,
        "chosen": lookup.chosen_check().map(|c| candidates[c.ranked.index].id),
        "split": split,
        "ms": ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestDb, model};

    #[tokio::test]
    async fn candidates_share_the_models_metadata_and_are_not_supported() {
        let t = TestDb::new().await;
        let meta = |m: &mut ModelRow, creator: Option<&str>, sub: Option<&str>| {
            m.calculated_creator = creator.map(String::from);
            m.calculated_collection = Some("Iron".into());
            m.calculated_subcollection = sub.map(String::from);
        };
        let mut sup = model("s.stl", "");
        meta(&mut sup, Some("FF"), None);
        sup.calculated_supported = Some(true);
        let mut twin = model("u.stl", "");
        meta(&mut twin, Some("FF"), None);
        let mut marked = model("m.stl", "");
        meta(&mut marked, Some("FF"), None);
        marked.calculated_supported = Some(false);
        let mut other_sub = model("o.stl", "");
        meta(&mut other_sub, Some("FF"), Some("Bulls"));
        let mut other_creator = model("c.stl", "");
        meta(&mut other_creator, Some("Loot"), None);
        let mut supported_peer = model("p.stl", "");
        meta(&mut supported_peer, Some("FF"), None);
        supported_peer.calculated_supported = Some(true);
        let mut copy = model("copy.stl", "");
        meta(&mut copy, Some("FF"), None);
        copy.checksum = sup.checksum.clone();
        let d = Descriptor { d2: [1.0; remesh_twin::D2_BINS], thick_area: 10.0, area: 20.0, spreads: [3.0, 2.0, 1.0] };
        for m in [&sup, &twin, &marked, &other_sub, &other_creator, &supported_peer, &copy] {
            t.insert_model(m).await;
            store_descriptor(&t.db, m.id, &d.to_bytes()).await.unwrap();
        }
        let root = Path::new("/models");
        let mut got: Vec<Uuid> = candidates(&t.db, &sup, root, None).await.unwrap().iter().map(|c| c.id).collect();
        got.sort();
        let mut want = vec![twin.id, marked.id];
        want.sort();
        assert_eq!(got, want);
        assert_eq!(candidates(&t.db, &sup, root, None).await.unwrap()[0].descriptor, d);
        // Only those described since.
        let later = Utc::now() + chrono::Duration::seconds(60);
        assert!(candidates(&t.db, &sup, root, Some(later)).await.unwrap().is_empty());
        // No metadata, no lookup.
        let mut bare = sup.clone();
        bare.calculated_creator = None;
        assert!(candidates(&t.db, &bare, root, None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_descriptor_rewritten_unchanged_keeps_its_time() {
        let t = TestDb::new().await;
        let m = model("a.stl", "");
        t.insert_model(&m).await;
        let at = || async {
            sqlx::query_scalar::<_, DateTime<Utc>>("SELECT described_at FROM model_remesh_descriptors")
                .fetch_one(&t.db)
                .await
                .unwrap()
        };
        store_descriptor(&t.db, m.id, &[1, 2, 3]).await.unwrap();
        let first = at().await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        store_descriptor(&t.db, m.id, &[1, 2, 3]).await.unwrap();
        assert_eq!(at().await, first);
        store_descriptor(&t.db, m.id, &[4]).await.unwrap();
        assert!(at().await > first);
    }
}
