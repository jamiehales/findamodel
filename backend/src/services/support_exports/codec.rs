//! The export entry file: a header naming what the entry was made from and by, then which of the
//! model's triangles are supports and the supports read from them as a [`SupportLayout`].
//!
//! Layout: `FMEX`, format version (u8), reader version (i32), detection version (i32), the key
//! (u64 length, UTF-8: the model file's checksum, scan config, supported metadata and whether the
//! detection was checked), then the export. Little-endian; floats are stored as their bits, so a
//! layout read back writes exactly the VOXL it was read from.

use std::collections::{BTreeMap, BTreeSet};

use crate::geometry::Vec3;
use crate::geometry::support_layout::{
    BallTip, BraceEnd, BraceLayout, ContactTip, KickstandLayout, PostLayout, SupportBase, SupportLayout, SupportPath,
};

/// What a pre-supported model exports as: None when it holds nothing to rebuild as supports,
/// else each triangle's support flag and the rebuilt supports.
pub type ModelExport = Option<(Vec<bool>, SupportLayout)>;

const MAGIC: &[u8; 4] = b"FMEX";
/// Increment when the encoding changes.
const FORMAT_VERSION: u8 = 1;

/// What an entry file says about itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub reader_version: i32,
    pub detection_version: i32,
    pub key: String,
}

pub fn encode(header: &Header, export: Option<(&[bool], &SupportLayout)>) -> Vec<u8> {
    let mut w = Writer(Vec::new());
    w.0.extend_from_slice(MAGIC);
    w.u8(FORMAT_VERSION);
    w.0.extend_from_slice(&header.reader_version.to_le_bytes());
    w.0.extend_from_slice(&header.detection_version.to_le_bytes());
    w.u64(header.key.len());
    w.0.extend_from_slice(header.key.as_bytes());
    match export {
        None => w.u8(0),
        Some((mask, layout)) => {
            w.u8(1);
            w.mask(mask);
            w.layout(layout);
        }
    }
    w.0
}

/// The header, read from the start of an entry. None when the bytes are not an entry in the
/// current format.
pub fn decode_header(bytes: &[u8]) -> Option<Header> {
    header(&mut Reader(bytes))
}

fn header(r: &mut Reader) -> Option<Header> {
    if r.take(4)? != MAGIC || r.u8()? != FORMAT_VERSION {
        return None;
    }
    let reader_version = i32::from_le_bytes(r.take(4)?.try_into().ok()?);
    let detection_version = i32::from_le_bytes(r.take(4)?.try_into().ok()?);
    let n = r.count(1)?;
    let key = String::from_utf8(r.take(n)?.to_vec()).ok()?;
    Some(Header { reader_version, detection_version, key })
}

/// The whole entry. None when the bytes are not a whole entry in the current format.
pub fn decode(bytes: &[u8]) -> Option<(Header, ModelExport)> {
    let mut r = Reader(bytes);
    let header = header(&mut r)?;
    let export = match r.u8()? {
        0 => None,
        1 => Some((r.mask()?, r.layout()?)),
        _ => return None,
    };
    r.0.is_empty().then_some((header, export))
}

struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u64(&mut self, v: usize) {
        self.0.extend_from_slice(&(v as u64).to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }
    fn vec3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }
    fn vec3s(&mut self, vs: &[Vec3]) {
        self.u64(vs.len());
        for &v in vs {
            self.vec3(v);
        }
    }
    fn f32s(&mut self, vs: &[f32]) {
        self.u64(vs.len());
        for &v in vs {
            self.f32(v);
        }
    }
    fn opt_f32(&mut self, v: Option<f32>) {
        self.bool(v.is_some());
        if let Some(v) = v {
            self.f32(v);
        }
    }
    fn opt_usize(&mut self, v: Option<usize>) {
        self.bool(v.is_some());
        if let Some(v) = v {
            self.u64(v);
        }
    }
    fn mask(&mut self, mask: &[bool]) {
        self.u64(mask.len());
        for bits in mask.chunks(8) {
            self.u8(bits.iter().enumerate().fold(0u8, |b, (i, &on)| b | (u8::from(on) << i)));
        }
    }
    fn tip(&mut self, t: &ContactTip) {
        self.vec3(t.contact);
        self.vec3(t.surface_normal);
        self.bool(t.from_outside);
        self.vec3(t.socket);
        self.f32(t.contact_diameter_mm);
        self.f32(t.body_diameter_mm);
        self.bool(t.ball.is_some());
        if let Some(b) = &t.ball {
            self.f32(b.diameter_mm);
            self.f32(b.cone_end_mm);
            self.opt_f32(b.depth_mm);
        }
    }
    fn brace_end(&mut self, e: &BraceEnd) {
        self.u64(e.support);
        self.opt_usize(e.kickstand);
        self.vec3(e.point);
    }
    fn diameters(&mut self, map: &BTreeMap<usize, Vec<f32>>) {
        self.u64(map.len());
        for (&k, d) in map {
            self.u64(k);
            self.f32s(d);
        }
    }
    fn set(&mut self, set: &BTreeSet<usize>) {
        self.u64(set.len());
        for &k in set {
            self.u64(k);
        }
    }
    fn layout(&mut self, l: &SupportLayout) {
        self.u64(l.supports.len());
        for s in &l.supports {
            self.tip(&s.tip);
            match &s.base {
                SupportBase::Plate { x, y, diameter_mm } => {
                    self.u8(0);
                    self.f32(*x);
                    self.f32(*y);
                    self.f32(*diameter_mm);
                }
                SupportBase::Shaft { parent, point } => {
                    self.u8(1);
                    self.u64(*parent);
                    self.vec3(*point);
                }
                SupportBase::Model(lower) => {
                    self.u8(2);
                    self.tip(lower);
                }
            }
            self.vec3s(&s.joints);
            self.f32(s.shaft_diameter_mm);
        }
        self.u64(l.posts.len());
        for p in &l.posts {
            self.vec3(p.foot);
            self.vec3s(&p.joints);
            self.vec3(p.top);
            self.f32(p.root_diameter_mm);
            self.f32(p.diameter_mm);
        }
        self.u64(l.braces.len());
        for b in &l.braces {
            self.brace_end(&b.lower);
            self.brace_end(&b.upper);
            self.f32(b.diameter_mm);
        }
        self.u64(l.kickstands.len());
        for k in &l.kickstands {
            self.u64(k.support);
            self.vec3(k.knot);
            self.vec3s(&k.joints);
            self.vec3(k.foot);
            self.f32(k.root_diameter_mm);
            self.f32(k.diameter_mm);
        }
        self.diameters(&l.segment_diameters);
        self.diameters(&l.kickstand_segment_diameters);
        self.set(&l.sticks);
        self.set(&l.twigs);
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u64(&mut self) -> Option<usize> {
        usize::try_from(u64::from_le_bytes(self.take(8)?.try_into().ok()?)).ok()
    }
    /// A count of items at least `min_bytes` long each, checked against what is left, so a
    /// damaged entry cannot ask for a huge allocation.
    fn count(&mut self, min_bytes: usize) -> Option<usize> {
        let n = self.u64()?;
        (n.checked_mul(min_bytes)? <= self.0.len()).then_some(n)
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_bits(u32::from_le_bytes(self.take(4)?.try_into().ok()?)))
    }
    fn bool(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
    fn vec3(&mut self) -> Option<Vec3> {
        Some(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    fn vec3s(&mut self) -> Option<Vec<Vec3>> {
        let n = self.count(12)?;
        (0..n).map(|_| self.vec3()).collect()
    }
    fn f32s(&mut self) -> Option<Vec<f32>> {
        let n = self.count(4)?;
        (0..n).map(|_| self.f32()).collect()
    }
    fn opt_f32(&mut self) -> Option<Option<f32>> {
        Some(if self.bool()? { Some(self.f32()?) } else { None })
    }
    fn opt_usize(&mut self) -> Option<Option<usize>> {
        Some(if self.bool()? { Some(self.u64()?) } else { None })
    }
    fn mask(&mut self) -> Option<Vec<bool>> {
        let n = self.u64()?;
        let bytes = self.take(n.div_ceil(8))?;
        Some((0..n).map(|i| bytes[i / 8] >> (i % 8) & 1 == 1).collect())
    }
    fn tip(&mut self) -> Option<ContactTip> {
        Some(ContactTip {
            contact: self.vec3()?,
            surface_normal: self.vec3()?,
            from_outside: self.bool()?,
            socket: self.vec3()?,
            contact_diameter_mm: self.f32()?,
            body_diameter_mm: self.f32()?,
            ball: if self.bool()? {
                Some(BallTip { diameter_mm: self.f32()?, cone_end_mm: self.f32()?, depth_mm: self.opt_f32()? })
            } else {
                None
            },
        })
    }
    fn brace_end(&mut self) -> Option<BraceEnd> {
        Some(BraceEnd { support: self.u64()?, kickstand: self.opt_usize()?, point: self.vec3()? })
    }
    fn diameters(&mut self) -> Option<BTreeMap<usize, Vec<f32>>> {
        let n = self.count(16)?;
        (0..n).map(|_| Some((self.u64()?, self.f32s()?))).collect()
    }
    fn set(&mut self) -> Option<BTreeSet<usize>> {
        let n = self.count(8)?;
        (0..n).map(|_| self.u64()).collect()
    }
    fn layout(&mut self) -> Option<SupportLayout> {
        let n = self.count(1)?;
        let supports = (0..n)
            .map(|_| {
                let tip = self.tip()?;
                let base = match self.u8()? {
                    0 => SupportBase::Plate { x: self.f32()?, y: self.f32()?, diameter_mm: self.f32()? },
                    1 => SupportBase::Shaft { parent: self.u64()?, point: self.vec3()? },
                    2 => SupportBase::Model(self.tip()?),
                    _ => return None,
                };
                Some(SupportPath { tip, base, joints: self.vec3s()?, shaft_diameter_mm: self.f32()? })
            })
            .collect::<Option<Vec<_>>>()?;
        let n = self.count(1)?;
        let posts = (0..n)
            .map(|_| {
                Some(PostLayout {
                    foot: self.vec3()?,
                    joints: self.vec3s()?,
                    top: self.vec3()?,
                    root_diameter_mm: self.f32()?,
                    diameter_mm: self.f32()?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let n = self.count(1)?;
        let braces = (0..n)
            .map(|_| Some(BraceLayout { lower: self.brace_end()?, upper: self.brace_end()?, diameter_mm: self.f32()? }))
            .collect::<Option<Vec<_>>>()?;
        let n = self.count(1)?;
        let kickstands = (0..n)
            .map(|_| {
                Some(KickstandLayout {
                    support: self.u64()?,
                    knot: self.vec3()?,
                    joints: self.vec3s()?,
                    foot: self.vec3()?,
                    root_diameter_mm: self.f32()?,
                    diameter_mm: self.f32()?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(SupportLayout {
            supports,
            posts,
            braces,
            kickstands,
            segment_diameters: self.diameters()?,
            kickstand_segment_diameters: self.diameters()?,
            sticks: self.set()?,
            twigs: self.set()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tip(x: f32, ball: Option<BallTip>) -> ContactTip {
        ContactTip {
            contact: Vec3::new(x, 1.5, 20.25),
            surface_normal: Vec3::new(0.0, -0.0, -1.0),
            from_outside: true,
            socket: Vec3::new(x, 1.5, 19.0),
            contact_diameter_mm: 0.4,
            body_diameter_mm: f32::from_bits(0x3f19_999b),
            ball,
        }
    }

    /// A layout using every field and variant, with floats a decimal round trip could change.
    fn sample() -> SupportLayout {
        let ball = BallTip { diameter_mm: 0.7, cone_end_mm: 0.0, depth_mm: Some(-0.05) };
        let mut layout = SupportLayout {
            supports: vec![
                SupportPath {
                    tip: tip(1.0, Some(ball)),
                    base: SupportBase::Plate { x: 1.0, y: 1.5, diameter_mm: 3.0 },
                    joints: vec![Vec3::new(1.0, 1.5, 4.0), Vec3::new(1.1, 1.5, 9.0)],
                    shaft_diameter_mm: 0.8,
                },
                SupportPath {
                    tip: tip(2.0, Some(BallTip { depth_mm: None, ..ball })),
                    base: SupportBase::Shaft { parent: 0, point: Vec3::new(1.0, 1.5, 6.0) },
                    joints: vec![],
                    shaft_diameter_mm: 0.6,
                },
                SupportPath {
                    tip: ContactTip { from_outside: false, ..tip(3.0, None) },
                    base: SupportBase::Model(tip(3.0, None)),
                    joints: vec![Vec3::new(3.0, 1.5, f32::MIN_POSITIVE)],
                    shaft_diameter_mm: f32::from_bits(1),
                },
            ],
            posts: vec![PostLayout {
                foot: Vec3::new(5.0, 5.0, 2.0),
                joints: vec![Vec3::new(5.0, 5.0, 3.0)],
                top: Vec3::new(5.0, 5.0, 8.0),
                root_diameter_mm: 2.0,
                diameter_mm: 1.0,
            }],
            braces: vec![BraceLayout {
                lower: BraceEnd { support: 0, kickstand: Some(0), point: Vec3::new(1.0, 1.5, 5.0) },
                upper: BraceEnd { support: 3, kickstand: None, point: Vec3::new(5.0, 5.0, 7.0) },
                diameter_mm: 0.5,
            }],
            kickstands: vec![KickstandLayout {
                support: 0,
                knot: Vec3::new(1.0, 1.5, 7.0),
                joints: vec![],
                foot: Vec3::new(-2.0, 1.5, 2.0),
                root_diameter_mm: 2.0,
                diameter_mm: 0.7,
            }],
            ..SupportLayout::default()
        };
        layout.segment_diameters.insert(0, vec![1.0, 0.8, 0.6]);
        layout.segment_diameters.insert(3, vec![1.2, 1.0]);
        layout.kickstand_segment_diameters.insert(0, vec![0.9, 0.7]);
        layout.sticks.insert(2);
        layout.twigs.insert(1);
        layout
    }

    fn header() -> Header {
        Header { reader_version: 3, detection_version: 6, key: "abc|scan|supported:Some(true)|checked:true".into() }
    }

    #[test]
    fn an_entry_reads_back_as_written() {
        let mask: Vec<bool> = (0..21).map(|i| i % 3 == 0 || i == 20).collect();
        let layout = sample();
        let bytes = encode(&header(), Some((&mask, &layout)));
        let (h, back) = decode(&bytes).expect("decodes");
        assert_eq!(h, header());
        assert_eq!(decode_header(&bytes), Some(header()));
        assert_eq!(back, Some((mask, layout)));
        // Floats come back bit for bit (PartialEq would let -0.0 equal 0.0).
        let (_, layout) = back.unwrap();
        assert_eq!(layout.supports[0].tip.surface_normal.y.to_bits(), (-0.0f32).to_bits());
        assert_eq!(layout.supports[2].shaft_diameter_mm.to_bits(), 1);
        assert_eq!(decode(&encode(&header(), None)), Some((header(), None)));
        let empty = SupportLayout::default();
        assert_eq!(decode(&encode(&header(), Some((&[], &empty)))), Some((header(), Some((Vec::new(), empty)))));
    }

    #[test]
    fn damaged_or_foreign_bytes_are_not_an_entry() {
        let layout = sample();
        let bytes = encode(&header(), Some((&[true, false, true], &layout)));
        for cut in [0, 3, 5, 6, 12, 20, bytes.len() / 2, bytes.len() - 1] {
            assert_eq!(decode(&bytes[..cut]), None, "cut at {cut}");
        }
        // The header alone is readable from a cut entry once it is whole.
        assert_eq!(decode_header(&bytes[..bytes.len() / 2]), Some(header()));
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(decode(&longer), None);
        let mut other_format = bytes.clone();
        other_format[4] = FORMAT_VERSION + 1;
        assert_eq!(decode(&other_format), None);
        assert_eq!(decode_header(&other_format), None);
        assert_eq!(decode(b"VOXL\x02\x00"), None);
        // A huge count is refused, not allocated.
        let mut huge = encode(&header(), None);
        *huge.last_mut().unwrap() = 1;
        huge.extend_from_slice(&0u64.to_le_bytes());
        huge.extend_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(decode(&huge), None);
    }
}
