//! The whole-tree support reader: reads a pre-supported model's supports from their mesh and
//! rebuilds them as a [`SupportLayout`](super::support_layout::SupportLayout) for VOXL export.
//! See `docs/support-reader.md` for every stage, rule and constant.

pub mod classify;
pub mod contacts;
pub mod math;
pub mod pieces;
mod union_split;

use crate::geometry::Triangle;
use crate::geometry::support_layout::SupportLayout;

/// Increment when the reader or the way its layout is written changes, so anything derived from
/// it (cached VOXL downloads) is rebuilt.
pub const CURRENT_SUPPORT_READER_VERSION: i32 = 3;

/// Shells reaching no higher than this are raft and give no contacts. The prototype measured the
/// suite (raft 3.0 mm) and the real models with 3.5 mm; a taller raft raises it.
pub fn raft_band(raft_height_mm: f32) -> f64 {
    (f64::from(raft_height_mm) + contacts::RAFT_BAND_MARGIN_MM).max(3.5)
}

/// The supports read and laid out.
#[derive(Debug, Clone)]
pub struct Reading {
    pub contacts: Vec<contacts::Contact>,
    pub pieces: pieces::Pieces,
    pub classification: classify::Classification,
}

impl Reading {
    pub fn layout(&self) -> &SupportLayout {
        &self.classification.layout
    }
}

/// Reads the supports (`supports`, Z-up) of `model` (Z-up, without its supports) and lays them out
/// as DragonFruit supports.
pub fn read(model: &[Triangle], supports: &[Triangle], raft_height_mm: f32) -> Reading {
    let model = math::points_of(model);
    let supports = math::points_of(supports);
    read_points(&model, &supports, raft_height_mm)
}

/// [`read`] on f64 corners.
pub fn read_points(model: &[[math::P; 3]], supports: &[[math::P; 3]], raft_height_mm: f32) -> Reading {
    let contacts = contacts::read(model, supports, raft_band(raft_height_mm));
    let at: Vec<(math::P, f64)> = contacts.iter().map(|c| (c.at, c.d)).collect();
    let pieces = pieces::read(supports, model, &at, pieces::RAY_PITCH_MM);
    let classification = classify::classify(&pieces, &contacts, model);
    Reading { contacts, pieces, classification }
}

/// A pre-supported model's export: which of its triangles are supports, its body (Z-up) without
/// them, and the supports read and laid out. None when nothing is split off, or when the
/// split-off parts are not supports (unless `checked`: a detection stored under the current scan
/// config was checked when the model was indexed). `triangles` are the model's loaded (Y-up)
/// triangles.
pub fn model_export(
    triangles: &[Triangle],
    raft_height_mm: f32,
    supported: Option<bool>,
    checked: bool,
) -> Option<(Vec<bool>, Vec<Triangle>, SupportLayout)> {
    use crate::geometry::{support_separation, to_z_up};
    let classification = support_separation::classify(triangles, raft_height_mm)?;
    let guard = !checked && !classification.generated;
    let mask = classification.mask_for(supported)?;
    if guard && !support_separation::rebuilds_as_supports(triangles, &mask, raft_height_mm) {
        return None;
    }
    let (body, supports) = support_separation::split(triangles, &mask);
    if supports.is_empty() {
        return None;
    }
    let body = to_z_up(&body);
    let reading = read(&body, &to_z_up(&supports), raft_height_mm);
    Some((mask, body, reading.classification.layout))
}
