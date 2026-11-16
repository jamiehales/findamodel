//! The whole-tree support reader: reads a pre-supported model's supports from their mesh and
//! rebuilds them as a [`SupportLayout`](super::support_layout::SupportLayout) for VOXL export.
//! See `docs/support-reader.md` for every stage, rule and constant.

pub mod classify;
pub mod contacts;
pub mod fused;
pub mod math;
pub mod pieces;
mod union_split;

use crate::geometry::Triangle;
use crate::geometry::support_layout::SupportLayout;

/// Increment when the reader or the way its layout is written changes, so anything derived from
/// it (cached VOXL downloads) is rebuilt.
pub const CURRENT_SUPPORT_READER_VERSION: i32 = 5;

/// Shells reaching no higher than this are raft and give no contacts. The prototype measured the
/// suite (raft 3.0 mm) and the real models with 3.5 mm; a taller raft raises it.
pub fn raft_band(raft_height_mm: f32) -> f64 {
    (f64::from(raft_height_mm) + contacts::RAFT_BAND_MARGIN_MM).max(3.5)
}

/// The fused reading of a solid ([`fused`]) replaces the straight one only when that routes fewer
/// than this share of the contacts: the Winged Hussars' welded trees route most of theirs and read
/// better straight; the Loot files' fused lattices route none or almost none.
pub const FUSED_ROUTED_SHARE: f64 = 0.5;

/// Contacts a classification routes (as tips or strut feet).
fn routed(c: &classify::Classification) -> usize {
    c.contacts.iter().filter(|s| matches!(s, classify::ContactState::Routed | classify::ContactState::Foot)).count()
}

/// The contacts as the fused reader takes them ([`fused::Seed`]).
pub fn seeds_of(contacts: &[contacts::Contact]) -> Vec<fused::Seed> {
    contacts.iter().map(|c| fused::Seed { at: c.at, d: c.d, dir: c.dir }).collect()
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
    read_contacts(model, supports, contacts, raft_height_mm)
}

/// [`read_points`] with the contacts given.
pub fn read_contacts(
    model: &[[math::P; 3]],
    supports: &[[math::P; 3]],
    contacts: Vec<contacts::Contact>,
    raft_height_mm: f32,
) -> Reading {
    let band = raft_band(raft_height_mm);
    let pieces = pieces::read_seeded(supports, model, &seeds_of(&contacts), band, pieces::RAY_PITCH_MM);
    let classification = classify::classify(&pieces, &contacts, model);
    // A solid the fused reader read too ([`fused`]) keeps the straight reading unless that routes
    // fewer than half the contacts and the fused one routes more.
    if let Some(fused) = pieces.with_fused() {
        let mine = routed(&classification);
        if (mine as f64) < FUSED_ROUTED_SHARE * contacts.len() as f64 {
            let other = classify::classify(&fused, &contacts, model);
            if routed(&other) > mine {
                return Reading { contacts, pieces: fused, classification: other };
            }
        }
    }
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
    use crate::geometry::support_separation;
    let classification = support_separation::classify(triangles, raft_height_mm)?;
    let guard = !checked && !classification.generated;
    let mask = classification.mask_for(supported)?;
    if guard && !support_separation::rebuilds_as_supports(triangles, &mask, raft_height_mm) {
        return None;
    }
    export_of_mask(triangles, mask, raft_height_mm)
}

/// [`model_export`] of a split already made (`mask`, a twin split's): the pieces closed where the
/// mask cuts a shell, and the supports read from them. None when nothing is split off.
pub fn export_of_mask(
    triangles: &[Triangle],
    mask: Vec<bool>,
    raft_height_mm: f32,
) -> Option<(Vec<bool>, Vec<Triangle>, SupportLayout)> {
    use crate::geometry::{support_separation, to_z_up};
    // Closed pieces: a welded split's seams capped ([`support_separation::welded::closed_split`]).
    let (body, supports) = support_separation::welded::closed_split(triangles, &mask);
    if supports.is_empty() {
        return None;
    }
    let body = to_z_up(&body);
    let reading = read(&body, &to_z_up(&supports), raft_height_mm);
    Some((mask, body, reading.classification.layout))
}
