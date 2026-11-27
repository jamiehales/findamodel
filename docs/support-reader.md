---
layout: default
title: Support reader
nav_exclude: true
---

# Support reader

The support reader (`backend/src/geometry/support_reader/`) reads the supports of a pre-supported
model from their mesh and rebuilds them as DragonFruit supports, which the VOXL writer
(`backend/src/geometry/voxl.rs`) writes into a DragonFruit (open-source resin slicer) scene. It is
used for single-model VOXL downloads (`routes/models.rs`) and plate exports
(`services/plate_export.rs`, through `support_reader::model_export`).

The supports are first split from the model by `support_separation.rs`. The old slice reader
(`support_layout::reconstruct`) is kept only as the separation guard
(`support_separation::rebuilds_as_supports`) that decides whether split-off parts are supports at
all; it no longer writes any layout.

The synthetic test cases, with pictures and current results, are in
[Support reader test cases](support-reader-tests.md); the cases cut from real models, checked
against their Lychee projects, are in
[Support reader real-structure test cases](support-reader-real-tests.md). The round-trip tool that builds the export
with DragonFruit and measures it is described in `backend/tools/support-reader/README.md`.

## Ground rules

These rules were set by the owner and apply to every stage.

- **Touch rule.** A support patch inside the model bounded by one intersection loop is a tip,
  whatever its shape. Tips through a thin wall and tips resting flush on the surface are tips. Side
  grazes and other patches bounded by several loops are clips, not contacts. A contact sits at the
  loop's centroid and is sized from the loop at DragonFruit's 0.05 mm tip offset. A blunt rod end is
  a large tip matching the rod.
- **What enters the model.** Only tips and micro supports may enter the model. There are no
  floating tips and no tips without a route down.
- **Whole tree first.** Every piece is listed and connected, and each connected structure is
  classified as a whole. Each tip follows its own trunk down; braces join trunks. Classification
  produces the final parts; export only writes them.
- **Fewer joints.** Fewer joints are preferred to a closer fit, as long as nothing intersects the
  model and the angles hold.
- **Headline measure.** Support volume overlap between the original supports and what DragonFruit
  builds from the export (see [Measures](#measures)).

## Stages

| Stage | Code | What it decides |
| --- | --- | --- |
| Contacts | `contacts.rs` | Where the original supports hold the model (the touch rule), each contact's place, width and direction. |
| Pieces and connections | `pieces.rs`, `union_split.rs` | Every closed solid of the support mesh, its fit (rod, cone, ball, bar or unread), splits of solids that are not one straight solid, which pieces overlap, abut or nearly touch, and which pieces hold each contact. |
| Preparation | `classify/prepare.rs` | One piece per straight stretch, inline tips cut off braces, pads cut off as raft, smoothed profiles, the raft and its top, which pieces stand on the plate, every piece's joins. |
| Routing | `classify/route.rs` | Each contact's piece, pads and sleeves, and the route of each tip down its own trunk to the plate, a footing on the model or a rod an earlier route owns. |
| The rest and roles | `classify/rest.rs` | Roots, posts, legs, braces, late sleeves, loose and floating pieces, and the role of every piece. |
| Second reading | `classify/reroute.rs` | Where the rest stage leaves a post, or rods hanging on a rod read as a brace: which routes took a brace for their own shaft, and the structure routed again with those told their way. |
| Shaping | `classify/tips.rs` | Each tip's contact, socket and widths, blunt and rounded ends, sphere-tip data and ball seats, shaft and root widths. |
| Layout | `classify/layout.rs` | The written lines, stepped tapers, narrowing beside the model, the surface normal at each contact, and the conversion to the writer's `SupportLayout`. |
| Writing | `voxl.rs` | DragonFruit entities: roots, trunks, branches, sticks, twigs, braces, kickstands and knots, with per-segment widths and the contact placement that makes the sliced loop match the original. |

`support_reader::read` runs the first stages in order; `classify::classify` runs preparation to
layout. The reader works in f64 (`math.rs`): its fits and ray tests compare distances of a micron
and less, which f32 cannot hold across a 200 mm plate.

Change `CURRENT_SUPPORT_READER_VERSION` (`support_reader/mod.rs`) whenever the reader or what the
writer writes for a layout changes: cached VOXL downloads (`services/voxl_cache.rs`) are keyed on
it, and export entries written by an older reader are not used (see below).

## Export entries

Reading the supports takes seconds to minutes per model, so each pre-supported model's export is
kept as an entry (`services/support_exports.rs`): which of its triangles are supports, and the
supports read from them. Model VOXL downloads and plate VOXL exports use it instead of reading the
supports again.

- Entries are files under `<data>/cache/support-exports/`, apart from the VOXL download cache, so
  downloads cannot evict them; nothing evicts them. A file is named by a hash of its key: the
  model file's checksum, the scan config (raft height and support detection version), the
  supported metadata and whether the detection was checked under the current scan config.
- The file is the authority. Its header (`FMEX`, format version, reader version, detection
  version, key) says what it was made from; the `models` row only indexes it
  (`support_export_path`, `support_export_version`). When the two disagree, the file wins.
- Indexing writes the entry of every pre-supported model with supports found (by metadata or a
  confident detection), from the detection's own split, as the `export` stage of the index run's
  timings. A rescan writes an entry again when its file is missing, damaged or written by a
  reader older than the minimum support export version (Settings; `AppConfig__MinimumSupportExportVersion`
  seeds it, and it stays put when the reader version is bumped). A current file the row does not
  index is only indexed.
- Exports use an entry only when it is at the current reader and detection versions and its key
  matches; otherwise they read the supports and write the entry at the current version, whatever
  the minimum. A model with supports split off but none read is stored as such; an unsupported
  model stores nothing.
- Old entries are deleted: when a model's row names a new entry (new content, raft height,
  supported metadata or detection check), the old file goes unless another model with the same
  content still names it, and the end of every full scan sweeps the files no row names that were
  last written over an hour ago (an export writes its entry before indexing it). Partial and
  single-model scans don't sweep. An export still reading a swept file keeps reading it (or, where
  the system refuses to delete an open file, the file waits for the next sweep).
- Installs from before entries existed have their minimum raised to 1 by migration 0011, so their
  next scan builds every missing entry.
- Per model, writing an entry costs the read itself (0.5 to 21 s at 4 threads on the sample
  models) and 7 to 450 KB on disk; using one costs under 0.25 s.

## Heights

All heights are measured from the plate (z = 0). DragonFruit stands every root on the plate, so its
root top (`ROOT_TOP`, 2.0 mm: a 0.5 mm disk under a 1.5 mm cone) is 2.0 mm above the plate whatever
the original's raft. Every rule that speaks of the root top or the root zone (below the root top)
uses that height, never the raft's top: the pads, the sleeves' feet, low braces, webs, the posts and
legs not written, and where a plate support's shaft starts. Two other heights are separate:

- the raft band of the contacts, from the model's raft height setting (see the touch rule);
- the raft's top (`raft_top`), the top of the pieces read as raft, which decides only what is raft
  and what stands on the raft, and where the round trip's overlap stops counting.

The owner chose the plate over the raft's top (2026-11-11): measuring pads, sleeve feet and low braces
from the raft's top made pieces up to 4.6 mm high on a 2 mm raft into ground, which lost braces
(real models 6b37ef0f and f03f9e3a, round-trip IoU +0.06 each with the plate; 48 models pooled
unchanged).

## Contacts: the touch rule

`contacts::read` finds where the original supports hold the model, by geometry alone.

- Every support shell's vertices are marked inside or outside the model by the generalised winding
  number (exact for nearby triangles, dipoles for far cells), so open and messy model meshes still
  have an inside.
- The inside part of each shell is grouped into connected patches, split where an intersection
  loop crosses them.
- A patch bounded by one loop is a touch: a tip, whatever its shape. A patch bounded by two or more
  loops is a pass-through. A shell inside the model with no loop is buried.
- Loops on a surface that does not separate inside from outside (a model's inner shell, a stray
  sheet) are not boundaries and are left out. Two loops of one patch at the same place and width are
  one loop on a doubled surface.
- A tip through a thin wall is a tip, its contact the entry loop: two small loops close together,
  with the support ending soon past the far one.
- A tip resting flush (a shell with no loop, the model within `FLUSH_GAP`) is a tip.
- A side graze (one elongated loop with the shell going on past both of its long ends) and any other
  multi-loop patch is a clip, not a contact.
- A pinhole beside a patch's loop is no exit: a loop under `NICK_SHARE` of the area of the patch's
  widest loop (a cone's point coming out of a neighbouring face by a hair) is left out, so the patch
  is a touch, not a part passing through (real cases t13_trunk_tip_missing, w03_many_shells,
  x16_brace_branch).
- Overlapping touches are one contact, sized by the union of their loops, when they are several
  shells of one tip entering the model at one place (a cone and its ball, welded rod ends): the
  narrower loop's centre lies inside the wider loop, and both loops lie in one face of the model
  (normals within 45 degrees, `SAME_FACE_COS`; neither centre further than `SAME_FACE_OFFSET` of
  the wider loop's radius off the other's plane). A nick beside a loop (under `NICK_SHARE` of its
  area) belongs to it whichever way it faces, and adds nothing to its size or place. Two tips side
  by side whose loops only overlap at their rims are two contacts (real cases p12 to p14, x14:
  centres 0.30 to 0.47 mm apart, loops 0.23 to 0.62 mm wide), and so are the two ends of a hair
  support in a gap or a crevice (m09_micro_as_branch, p06_tip_grazing_missed). A contact sits at
  its loops' area-weighted centre, with the width of a circle of its area, and a direction away
  from the model along the support.
- Shells reaching no higher than the raft band (`raft_band`: the raft height plus
  `RAFT_BAND_MARGIN_MM`, at least 3.5 mm) are raft and give no contacts, unless they start more
  than `RAFT_CLEAR_MM` above the plate: such a shell stands on the raft or on a support (the tip
  cone of a trunk holding the model just above the raft, t14_trunk_tip_in_raft_band) and its loops
  are judged; it is never taken as resting flush.

| Constant | Value | Why |
| --- | --- | --- |
| `JOIN_GAP` | 0.001 mm | Open loop ends this close are joined (numeric cracks in a body). |
| `MIN_LOOP_AREA` | 1e-6 mm2 | A smaller loop is float noise. |
| `SIDE_STEP` | 0.005 mm | The two sides of a loop's model face are tested this far off it, to tell a separating surface from one that is not. |
| `FLUSH_GAP` | 0.03 mm | A shell with no loop this near the model is a tip resting flush. |
| `ELONGATED` | 2.5 | A loop this much longer than round (perimeter squared over 4 pi area) is elongated, the first test for a graze. |
| `THIN_WALL_SPACING` | 2.0 mm | A pass-through's two loops at most this far apart ... |
| `THIN_WALL_LOOP_D` | 1.5 mm | ... each at most this wide ... |
| `THIN_WALL_END` | 3.0 mm | ... with the support ending within this past the far loop, is a tip through a thin wall. |
| `COINCIDENT_MM` | 0.03 mm | Two loops of one patch this close in centre and width are one loop on a doubled surface. |
| `RAFT_BAND_MARGIN_MM` | 0.5 mm | Shells no higher than this above the raft's top are raft and are not judged. |
| `RAFT_CLEAR_MM` | 0.3 mm | A shell in the raft band starting this far above the plate is not raft. |
| `NICK_SHARE` | 0.05 | A loop under this share of a larger loop's area is a nick: no exit from its patch, no contact of its own. Pinholes seen are under 0.2%; the narrowest second loop of two real tips is 27%. |
| `SAME_FACE_COS` | 0.7 | Loops of one contact have normals within 45 degrees (a hair across a crevice: 60 degrees). |
| `SAME_FACE_OFFSET` | 0.5 | ... and centres within this share of the wider loop's radius of each other's plane (a hair across a 0.13 mm gap: 1.8). |
| `FINE`, `COARSE`, `FAR` | 2, 16, 3 | Winding number cells: exact within, dipoles beyond `FAR` cell radii. Speed only. |
| `COL` | 2 mm | The model's column grid for box and depth queries. Speed only. |

## Pieces and connections

`pieces::read` turns the support mesh into pieces and measures how they connect.

- **Weld and dedupe.** Vertices within 0.05 micron (`WELD`) are merged; degenerate and repeated
  triangles are dropped.
- **Closed solids.** Triangles are grouped by shared edges. At a non-manifold edge (more than two
  faces) faces are first paired flat-first: two faces in one plane through the edge, of opposite
  winding, each the other's unique flat partner. A stack pairs the rest. Flat-first is skipped where
  reversed twin end caps end at the edge (joined rods stay two solids), and where two solids meet
  flush along a face that ends at the edge (rods cut into sections whose caps weld). A closed group
  made only of reversed twin faces (the zero-volume sheet between two joined rods) is dropped.
  Motivating cases: z01_chain, z05_band25, z06_brief10, f01_hub.
- **Fit.** Each solid is read as a solid of revolution about one straight axis (a profile of rod,
  cone and ball sections, the axis found from where the face normals' lines meet), a ball, a bar (a
  prism of any cross-section, read as a rod of the same length and area), or kept as an unread piece
  with its principal axis, ends and width. Nothing is dropped.
- **Splitting.** A solid that is not one straight solid is split into straight stretches. That is an
  unread solid, a bar of more than 64 faces, or a solid of revolution whose profile describes more
  than 3 times or less than a third of its volume (an axis far off).
  - First as a welded union of regular rods (`union_split.rs`): the facets of one ring of an n-gon
    rod or cone all make the same angle with the axis and lie at the same offset from it, so three
    facets in a row give the axis, and every facet fitting it joins that rod.
  - Else by straight stretches (`split_straight`): flat side strips joined along their long edges,
    ended by a crease across them (a mitre ring, the loop where a leg meets a shaft), one fit per
    stretch, collinear stretches merged. The split is kept when there are two or more stretches and
    they cover at least half of the solid's surface, not counting plates (flat faces over
    `PLATE_FACET_MM2`, 1 mm2, that are no strip: the pads and raft of a support tree welded into one
    shell, as Factory Fortress' Winged Hussar ed34ba2a has).
  - Motivating cases: p05_bent30, p05_bent60, r03_padshell, welded variants, and real model c550facb.
  - A fused support side (below) when the solid is no welded union of regular rods and holds more
    than `fused::MIN_TRIANGLES` triangles or at least `fused::MIN_CONTACTS` contacts. Straight
    stretches read such a solid slowly (minutes on 0.1M triangles) and badly (nothing or almost
    nothing on the Loot files). The fused reader gives the stretches and their joins; the
    classifier routes and lays them out as any split solid's.
- **Profile steps.** A pad welded under a post keeps its step in the profile; it does not taper.
- **Connections.** Rays along the three axes on a jittered lattice (`RAY_PITCH_MM`) give exact
  solid intervals per ray. Two pieces overlap, abut (intervals ending and starting within
  `ABUT_MM`) or are a near miss (a gap under `NEAR_MM` with nothing between), each with its
  centroid and overlap volume. The same rays give each piece's volume inside the model.
- **Contact holders.** Each contact's pieces, by majority parity along three slightly tilted rays
  (`RAYS`, two up and one down). A single straight-up ray missed polygon cones whose edges lie along
  it (h03_vee at 8 sides, 45 degrees). The nearest pieces within 1.5 times the loop radius plus
  0.3 mm are listed too, up to six per contact.

| Constant | Value | Why |
| --- | --- | --- |
| `WELD` | 1/20000 mm (0.05 micron) | Vertices this close are one: exporters write one point with float noise. |
| `RAYS` | three generic directions | Contact holder parity rays, so no ray runs along a polygon's edges. |
| `ABUT_MM` | 0.02 mm | Solids ending and starting this close along a ray touch. |
| `NEAR_MM` | 0.3 mm | A gap this small with nothing between is listed as a near miss (a weak join). |
| `RAY_PITCH_MM` | 0.025 mm | Connection ray spacing: a 0.1 mm hair support meets a dozen rays along each axis. |
| `TILE` | 128 rays | Rays per tile side. Speed only. |
| `MIN_RING` | 4 facets | Facets of distinct normals a ring must show to be a rod's, in a welded union. |
| `TOL_S` | 0.002 | Tolerance on a facet's angle to the axis (`n.d`) in a welded union. |
| `TOL_K` | 0.002 mm | Tolerance on a facet's offset from the axis in a welded union. |

### The fused reader

`fused.rs` reads a support solid welded into one shell (the support side of a twin or welded split:
rods, braces, raft and tips one solid) from its horizontal slices.

1. **Slices** every `DZ` (0.1 mm). Each crossing triangle gives a segment, chained into loops by mesh
   edge (exact). Outer loops are blobs, inner loops their holes.
2. **Blob graph.** Blobs of consecutive slices that overlap are linked (the solid's Reeb graph of
   height).
3. **Walks.** Each contact facing down or sideways seeds a walk in the blob just under its loop. The
   walk follows the rod's core down: the mean-shift centre of the blob within `CORE_R` (1.15) of
   the rod's radius of where the line predicts it, so a brace or branch welded on one side does not
   pull it. At a fork it takes the blob holding the predicted core. It ends on the ground (the raft
   band, or a blob over 30 mm2 within 6 mm above it), on a contact facing up (`FOOT_REACH_MM`: a
   strut, `SupportBase::Model`), or at a dead end (dropped).
4. **Tree.** Walks ending on one ground blob are a trie up from it. At each fork the child that best
   continues the line below is the trunk; the others are branches (`SupportBase::Shaft`) at that
   blob.
5. **Widths at the core.** Twice the median distance from the core to the blob's edge over
   `WIDTH_RAYS` (16) directions, corrected for lean. A weld on one side lengthens a few rays, not
   the median, so welds do not inflate the width as the blob's area does. The shaft is the median
   over the support's own nodes; the socket is where the width first reaches 85% of it; the tip is a
   cone from the contact to the socket.
6. **Braces only from the solid's leftover mesh.** Blobs no walk owns join into components. A
   component standing on the ground and touching no support is a post. Otherwise its attachments to
   supports are paired by the way each leaves towards the other. Blobs two supports' walks share
   over at most 1.2 mm of height are a level bar, except within `ROOT_ZONE_MM` (2.5 mm) above the
   raft band, where touching cone feet merge blobs. Every brace's centre line must lie inside the
   solid (`MATERIAL_SHARE`, 90% of its samples every 0.1 mm inside a blob of their slice), so none
   crosses an X lattice's empty space. Attachments left unpaired are counted as dangling and
   written as nothing.
7. **Out.** Each support's line (contact, socket, bends at `STRAIGHT_MM`, base) becomes straight
   stretches (a cone then rods), each brace and post a rod, with their joins given explicitly
   (`Piece::segment_links`): consecutive stretches, a branch's foot on its parent's stretch, a
   brace's ends on their supports. `split_pieces` takes those joins instead of joining stretches
   whose centre lines meet.

The reading is deterministic: loops start at their smallest edge id, blobs are sorted, maps are
walked in key order and every walk runs on its own (checked on 1 and 4 threads).

| Constant | Value | Why |
| --- | --- | --- |
| `DZ` | 0.1 mm | Slice spacing. At 0.05 mm the read took twice as long and fitted worse (walk windows are counted in slices); at 0.2 mm the 0.2 mm tips blur. |
| `MIN_TRIANGLES` | 20,000 | A solid this big that no union of regular rods reads is a fused support side. |
| `MIN_CONTACTS` | 6 | So is a smaller one holding this many contacts. |
| `WIDTH_RAYS` | 16 | Directions of the core width. |
| `ROOT_ZONE_MM` | 2.5 mm | No level bar this close above the raft band (touching cone feet). |
| `MATERIAL_SHARE` | 0.9 | Share of a brace's centre line inside the solid. |

## Preparation

`classify/prepare.rs` makes the pieces ready to classify (`load` runs split stretches, inline tips,
split stretches again, then pads).

- **Split stretches** (`split_pieces`). A split solid becomes one piece per stretch. Its joins and
  contacts go to the stretch nearest them, except that a join goes to every stretch whose axis passes
  within both radii (plus 0.05 mm) of the other piece: a welded X's join with a post, which both its
  arms enter, including in the root zone where the other piece comes down into it from above
  (x06_close20). Stretches of one solid join each other where their centre lines come within their
  radii plus 0.3 mm. Low wide solids (wider than 6 mm, or both under the root top plus 0.5 mm) keep
  the nearest rule.
- **Inline tips** (`split_inline`). A leaning tip cone welded in line with a brace that carries on
  past a post's top reads as one straight solid. Where the cone's wide end meets the top of a
  near-vertical piece going down, and the solid goes on down at a slant, it is cut there: the tip
  follows the post down and the rest is a brace (l02_lean_cone welded). A tip welded to its own
  shaft is left whole.
- **Pads** (`split_pads`). A wide pad on the plate in one shell with the post standing on it is cut
  at the pad's top: the pad is raft, the rest stands on it. The solid is a solid of revolution
  starting on the plate (within `PLATE_Z` + 0.05 mm) with its axis within about 14 degrees of
  vertical; the pad is its first sections whose wider end is at least `RAFT_MIN_XY` across (a ball
  section ends it), its top no higher than `PAD_BAND` above the plate; the rest is at most half as
  wide as the pad's top section at its narrower end and at least 1 mm long.
- **Smoothing** (`smooth_secs`). An 8-sided welded shaft split by the pieces stage comes as many short
  rod and cone sections whose radius wobbles between the prism's inner and outer radius; the first
  dip would read as a tip cone. Runs of 6 or more rod and cone sections are simplified
  (Douglas-Peucker on the radius, `PROFILE_TOL`); shorter profiles (every turned Chitubox part) stay
  as read.
- **Raft.** A piece on the plate (reaching down to `PLATE_Z`) at least `RAFT_MIN_XY` wide, at most half
  as tall as wide and topping out under 6 mm, is raft. Flat pads on the plate (under 1.5 mm tall)
  that join into a sheet at least `RAFT_MIN_XY` across are raft (a raft laid as tiles). The raft
  grows by flat pieces at least 4 mm wide lying on it. Anything topping out under the raft's top is
  raft.
- **Plate.** A piece reaching the plate, standing flush on the raft's top (within 0.05 mm), or joined
  to the raft and starting within 0.5 mm of its top stands on the plate.
- **Joins.** Every overlap, abutment and near miss between non-raft pieces becomes a join on both
  pieces, at its place along each; near misses are weak joins, used only in the second routing pass.
- An unread piece is used as a straight stretch between its ends at its width.

| Constant | Value | Why |
| --- | --- | --- |
| `PAD_BAND` | 3.0 mm | A pad welded under a post counts as raft up to this height above the plate. |
| `PROFILE_TOL` | 0.06 mm | Profile points within this of a straighter profile are polygon noise. |
| `PLATE_Z` | 0.05 mm | A piece reaching this low stands on the plate. |
| `RAFT_MIN_XY` | 8.0 mm | A piece on the plate at least this wide is raft. |
| `ROOT_TOP` | 2.0 mm | DragonFruit's root top (a 0.5 mm disk under a 1.5 mm cone); below it is the root zone. |

## Routing

`classify/route.rs` first settles each contact's piece, the pads and the sleeves, then routes.

- **Pads.** Pieces on the plate or raft wholly under the root top (with 0.6 mm slack, so no higher
  than 2.6 mm above the plate whatever the raft), holding no contact: a foot cone, a disc under a
  support. They are the ground: what stands on one stands on the plate (a piece joined to one at
  most 1.0 mm above the root top and starting at most 0.6 mm above it), and no route runs through
  one. A joint ball on a pad passes "on the plate" on to the
  pieces on it (a short trunk read as pad, ball, tip; real model 03a1f70b).
- **Sleeves** (`find_sleeves`). A piece is part of its host, and never routed, when all of these
  hold: it lies along another support piece within `SLEEVE_COS` (5 degrees), its centre line within
  `SLEEVE_OFF` of the host's radius, at least `SLEEVE_SHARE` of its volume shared; it lies wholly in
  the host's span but for a foot in the root zone; its top reaches another piece; and it holds no
  contact. The thresholds come from 6 real models: sleeves have centre lines within 0.02 to 0.25 of
  the host's radius and share 0.73 to 1.00 of their volume; the nearest other pieces share at most
  0.53. Cases s12 to s15; real models a4278c33, e1c9688c.
- **Contact pieces** (`contact_piece`). Each contact's piece is one holding it by parity, a rod or
  cone before a ball, then the one whose end is nearest. If no piece holds it by parity, a cone or
  rod ending there (within 0.6 mm of its end) with the contact within 0.85 of its read radius does
  (`in_profile`). The parity fix removed the synthetic need for this, but 16 real contacts lie just
  outside cone ends, and 6 of 62 real layouts change without it, so it stays. Every non-ball piece
  the contact sits in (or within its radius plus 0.05 mm) can end a route there (several rods
  entering one loop).
- **Order.** Contacts whose piece leads down or level are routed first, highest first, each lowered
  by `DETOUR_W` times its route's detour (length less the height it drops, from a first walk with
  nothing claimed): the tip that goes most straight down owns a shared trunk (micro cases). A tip's
  own cone stays its own: a tip whose first walk joins the cone holding another tip in the cone's
  body (past that tip's contact, where the cone is wider than at the contact and still under
  `CONE_BODY` of its full width) is routed after that tip, so it hangs from the cone instead of
  taking the cone's line down and leaving its owner a stub (a mini support standing on a tip cone,
  real case l04_leaf_on_strut). A rod is its own tip's in the same way: a tip whose first walk joins
  a rod from the side (further than the rod's radius, at least 0.3 mm, from either of its ends),
  where another tip's first walk comes down that rod from its end and on past the join, is routed
  after that tip, so it hangs from the rod as a branch instead of owning the rod's lower part and
  leaving the rod's own tip a stub with a joint more (real cases h03_hollow_branch,
  x18_brace_struts). Contacts
  on upward faces (their piece leads up past `LEVEL`) follow, as feet; they keep to their own rod
  rather than the way most nearly down. A contact that finds no route is tried again in later rounds
  (up to 4) once more routes exist to hang from.
- **Walking.** A route runs along its piece to the end, and at each joint takes the way that best
  continues it: joints nearest the end first; then the same rod going on (below); then ways within
  25 degrees of straight down (`NEAR_VERTICAL`), then sloping down, then level (`LEVEL`), then a rod
  an earlier route owns, then up; within a class a rod that starts at the joint before a rod passing
  by it (the joint further than the rod's radius, at least 0.3 mm, from both of its ends), then the
  least turn, with a rod beside this one's line counting 40 degrees per radius it stands off. A
  leaning tip on the top of its own pillar keeps that pillar though a taller neighbour passes nearer
  the cone's end (pillars 0.97 mm apart, real case n9a_kickstand_opr). A rod standing off this rod's
  end by over a radius, with another tip's own piece on its end there, is that tip's shaft: a way
  down it ranks a class lower, after the ways down the rods that meet the end (a tip beside a strut
  took the strut's steeper shaft and left its own rods as braces, real case b14_branch_on_strut).
  Without the other tip the rule sent routes down braces on whole models (2d809f62, 891abe26). Another tip's own piece
  passing by the joint (its cone, where tip cones lap each other at their sockets) is that tip's
  from the start: it ranks with the rods an earlier route owns, after every way down or level
  (real model 35ba31e3, where routes ran down a chain of each other's cones). Never straight back (a turn over 150 degrees), and never up while anything else goes
  on. Joint balls are hubs: a route passes through a ball to the rods on it. A contact piece too
  short to have a direction (a 0.15 mm tip ball stub) is left by its joints, like a ball. Three
  passes: strict, then with near misses, then also going up; the pass is reported.
- **The same rod going on.** A rod read in two pieces is one rod: at a rod's end, a rod of the same
  width where they join (`SAME_ROD_WIDTH`) on the same line (`SAME_ROD_DEG`, `SAME_ROD_OFF`) is
  taken before every other way. The rods may taper (a mini support in two pieces,
  l03_leaf_on_branch). Lychee draws a branch as a rod from the socket and a second rod, lapping
  it, from the parent's centre line; without this rule the route left the upper rod where it grazes
  the parent, the knot sat up to 2 mm high and the lower rod was read as a brace (real cases
  n3b_the_witchguild_braces, b01_branch, b09_branch_route, n1a, n5a, n8a). A piece holding a
  contact (a tip's own piece) is never the same rod as what it stands on, and a rod that carries on past a way straight down is not the same
  rod but a brace in line with it (a leaning shaft on a post with a brace going on to the next post,
  l01_lean_shaft, l03_lean_ball).
- **Ends.** A route ends at the plate (a piece standing on it), on a rod an earlier support, post or
  leg owns (a branch, at the attach point), or on a footing on the model: another contact on the
  same piece, ahead of it and within that contact's depth plus 0.6 mm of the piece's end (the route
  becomes a strut and that contact its lower tip).
- **Foot versus through.** A rod ending in the model ends the route there, unless the structure goes
  on down from that joint (a stray sheet of the model across a joint; a real foot has nothing
  hanging below it). A way down into another rod ending in the same loop is not going on (a vee: two
  struts or branches whose feet meet at one loop) when all of these hold:
  - both pieces hold the contact;
  - their centre lines cross no more than `VEE_HUB_MM` past it (else it is a hub inside the model,
    k03_modeltree);
  - the other rod's end is within the contact's depth plus 0.6 mm of the loop;
  - the two rods diverge by more than 8 degrees (`VEE_MIN_COS`): real vees are 12.6 to 39 degrees
    apart, h03_vee 16.7, while stacked collinear pieces of one strut (the hollow-strut models
    0e4bfe5b and 8447ddb6) are not a vee;
  - the other rod leads, within 8 joins, to a contact of its own, so its own route builds it.
- **Through contacts.** A contact on a piece a route runs through (not at either end of it) is passed
  through: a routed support runs on through the model there, so no tip is built. So is a contact
  where the route's flank lies in the model: inside a piece of the route, its own piece a collar
  round the route (joined to nothing but the route's pieces, with no other contact inside it: the
  sleeve of a mini support lying in the surface, real case m04_micro_extra_tip). Such a flank loop
  overlapping the loop of one of the route's own ends is part of that end's opening, and the end is
  sized by both (the two circles' union). Passed contacts are reported as exceptions, as is every
  contact left without a route (with what its structure offers).
- **Second reading** (`classify/reroute.rs`, after the rest stage). Each route takes the best way on
  at every joint without knowing what the other routes need, so two rods that look alike at a joint
  are taken for each other: a tip's own shaft and a brace ending at its socket. The rest stage then
  shows the mistake, and the structure is routed again with the routes concerned told which way to
  take:
  - a post (a pillar on the plate with no tip) with a rod read as a brace leaving its top upwards:
    the knee of a trunk whose route went down another rod (real case p10_tip_on_edge_size);
  - a leg with a knee (an upright pillar and its lean, see Knees below) whose lean ends at a
    route's socket: the same knee of a trunk, which the rest stage reads as a kickstand under the
    trunk's tip since legs have knees (real case x12_brace_steep). A kickstand that leans on a
    support's shaft, not on its socket, is left alone;
  - a rod with both ends on one support: another way down for that support's route;
  - a rod whose end hangs on a rod read as a brace: a brace cannot host it, a support's shaft can
    (real case x16_brace_branch, where the branch's shaft and a brace leave the socket 45 degrees
    down each and only the two braces ending on the shaft tell them apart).

  From such a rod the line is followed through spare rods (up to 35 degrees off, as a brace's run)
  to the socket of a route that leaves its tip piece another way; that route is told to take the
  rod there. Only a socket counts: there a tip's shaft and a brace look alike, while a joint
  further down a route moved the knots of branches along the braces beside their parents (real
  model 01a23637) and stood branches on the pillars of posts that are not theirs.
  A route that runs on from that joint along one straight line to the plate or to its foot on the
  model (within `STRAIGHT_ON_COS`, a foot's own tip piece aside) keeps its way: a pillar, a strut's
  span (real model 01a23637, where struts were sent down the braces beside them). A piece holding a
  contact is never routed through (a collar or a flank in the model, m04_micro_extra_tip).

  A second reading is kept when it leaves fewer pieces unread (the pieces of posts and legs, loose
  and floating pieces) and no more contacts without a route. Legs count as unread so that a trunk
  is never given up for the kickstand leaning on it. Fewer joints beats a closer fit, so a route
  told a way may not come out with over one bend more than it had (bends as the layout keeps
  them), or over two when it now stands on the plate and did not before (the socket and the knee
  of a trunk first read as a stub hanging from its neighbour); a route that does was sent
  wandering through a lattice of braces (real model 3de1d1a4) and goes back to its own way, as do
  the routes beside whatever is newly unread when the reading is no better. Then the rest is read
  once more. At most `REROUTE_ROUNDS` rounds of wishes, each read at most that many times.
- **Not ported.** The prototype's opt-in switches for tips at Chitubox joint balls
  (`WELDED_SAMEFOOT`, `WELDED_FOOTPOS`) and for hair micros sharing one foot (`MICRO_SHARED_FOOT`)
  were off and are not in the port: each hair keeps its own support and lower tip, and tips at
  Chitubox joint balls are fixed in the separation instead.

| Constant | Value | Why |
| --- | --- | --- |
| `NEAR_VERTICAL` | cos 25 degrees | A way within 25 degrees of straight down is "down". |
| `LEVEL` | 0.15 | A way whose vertical component is within this of level is level; a contact piece leading up past it routes as a foot. |
| `SLEEVE_COS` | cos 5 degrees | A sleeve runs within 5 degrees of its host's line ... |
| `SLEEVE_OFF` | 0.3 | ... its centre line within this share of the host's radius ... |
| `SLEEVE_SHARE` | 0.6 | ... with at least this share of its volume inside the host (owner: stays 0.6 for partly shared rods). |
| `DETOUR_W` | 0.5 | Routing order: height lowered by this share of the route's detour, so the most direct tip owns a shared trunk. |
| `VEE_MIN_COS` | cos 8 degrees | The two foot cones of a vee are at least 8 degrees apart. |
| `VEE_HUB_MM` | 0.25 mm | Two foot cones in one loop are a vee unless their lines cross this far past the contact (a hub inside the model). |
| `CONE_BODY` | 0.9 | A route joining another tip's cone where it is under this share of its full width joins it in the cone's body, and is routed after that tip. |
| `SAME_ROD_DEG` | 5 degrees | A rod read in two pieces: the second lies within this of the first's line ... |
| `SAME_ROD_OFF` | 0.3 | ... the first's end within this share of the thinner one's radius of the second's line ... |
| `SAME_ROD_WIDTH` | 0.15 | ... and their radii where they join within this share of the wider one's. |
| `REROUTE_ROUNDS` | 3 | Rounds of a second reading, and readings per round. |
| `STRAIGHT_ON_COS` | cos 20 degrees | A route whose stretches from a joint on turn no more than this runs straight on, and keeps that way in a second reading. |

## The rest and roles

`classify/rest.rs` handles every piece no route uses, then gives every piece its role.

- **Roots.** A short free piece on the plate under a support that reaches the plate (its foot cone or
  pad) is that support's root. Root pieces under a post or leg widen that post's or leg's root.
- **Base stubs** (`base_stubs`). A free piece on the line of a routed rod it joins (within
  `SLEEVE_COS`, both ends within `SLEEVE_OFF` of the rod's radius of its line), running on past
  one end of that rod by more than `STUB_PAST_MM`, is that rod's own base: part of its support (a
  sleeve), never a brace or a leg. Lychee builds a branch's base as a short capsule from the knot
  on the host, overlapping the branch's shaft along its line; the route joins the host from the
  shaft and leaves the capsule to no route (real cases b01_branch, b05_branch_depth3,
  b09_branch_route, e02_elf_branches: 0.4 to 0.7 mm of overlap, lines 0.00 mm apart). The far end must lie
  in a piece of the same support or of the support the rod's own hangs from; no higher than
  `PAD_START_MM` above the root top (where a piece standing on a pad may start) the base of the
  last rod of a support on the plate is its root whatever it lands in (b03_tree).
  A brace carrying on in a branch's line to another support ends elsewhere and stays a brace
  (l01_lean_shaft).
- **Posts and legs.** Free pieces standing on the plate are followed straight up through free pieces
  (upright pieces first, so a post is claimed before a leg leaning on it looks for its host). A run
  whose top leans on a support or post is a leg (a DragonFruit kickstand); otherwise a post. Where
  the top meets a support or post and a leg already leaning there, it leans on the support or
  post (two legs from one foot pad, each onto its own trunk: real model 31676804, where the second
  leg was a post, or a brace from the first leg's root zone that DragonFruit knots 1.5 mm off its
  line). A leg
  cannot lean on another leg (DragonFruit's kickstands hold up supports and posts only); it stands
  as a post of its own, unless its low end itself joins a support: then it is left to be a brace
  from that support to the leg (x17_brace_kickstand). One that only stands on a support's root pad
  stays a post (as a brace it would have one host and be written as nothing; real models 2b73cb36,
  af84e884). A run whose
  low end joins a routed support is a brace, not a leg, only when it stands on the raft or lies
  wholly under the root top (a slanted brace landing on a pad at a post's foot; bent I12,
  r05_padfoot).
- **Knees** (owner, 2026-11-25). Where nothing goes straight on from a run's top and one free piece
  alone leans up off it (joined at the top end, rising, turning by at most `KNEE_DEG`), the run
  turns into it: an upright pillar and its lean onto a support are one leg with a knee, as a Lychee
  kickstand is and as DragonFruit builds its own (b12_kickstand, e01_elf_kickstand,
  x17_brace_kickstand), not a post with a brace on its top. The pillar's stretch ends where the
  lean's line crosses its own (the knee), not at its top: a cap above the knee (0.1 to 0.75 mm on
  b12_kickstand, e01_elf_kickstand and x12_brace_steep) would bend the line back on itself. A lean
  that reaches no support or post is no part of the run (the pillar stays a post and the lean a
  brace, as before); where the lean's top meets a piece still waiting to become a post, the run
  waits for it once. The layout writes the knee as a joint of the kickstand
  (`KickstandLayout::joints`), which the VOXL writer, the export entries and the nominal build
  already carry.
- **A branch off a leg's pillar** (owner, 2026-11-26). An upright piece on the plate that a
  support owns from its foot up, with a top stretch of at least 1 mm (and its own width) that no
  route uses (a branch leaves it lower down), can still turn through a knee onto another support
  or a post: the pillar's whole length and the lean are then one leg, standing in the same place
  as the support that climbs the pillar's lower part. DragonFruit's kickstand carries no branch,
  so a Lychee branch hanging from a kickstand is written as a support of its own up the pillar
  and the kickstand whole beside it, the two coinciding from the root top to where the branch
  leaves (b13_branch_on_kickstand: before, the lean was a brace and the pillar between the branch
  and the knee, 3.5 mm, was not built). Without a knee onto another host the pillar stays as it
  was.
- **A pillar at a support's foot.** One upright piece from the plate (within `FOOT_PILLAR_COS` of
  vertical) that meets a support standing on the plate at that support's own last piece, inside
  the pillar's width and no higher than `PAD_REACH_MM` above the root top, is that support's root,
  not a leg holding it up: the upright under a tip cone that starts in the raft band, which is
  0.02 mm too tall to be a pad (t02_trunk_wall_raft). A slanted rod from one support's root zone
  up to another support stays a leg (p01_tip_tiny, x09_brace_foot).
- **Braces.** Free pieces join end to end into straight runs (each end goes on into at most one
  piece, the one that best continues it within 35 degrees, and only when that piece agrees). A run
  is a brace between each pair of neighbouring hosts along it (a support, post or leg), its ends
  where its centre line passes the host's. A run with both ends below the root top is a web in the
  raft band, not a brace (z cases). A run on one support only is not writable as a brace. A run
  from a branch to the support it hangs from, with both ends within `KNOT_REACH_MM` of the branch's
  knot, is a second short way onto the same host (the base and short shaft of a Lychee branch whose
  cone reaches its host itself: b02_branch_fan, p13_tips_close_missed, 0.7 to 1.2 mm long), the
  branch's base and not a brace. Only a run between those two hosts alone: a run that goes on to a
  third host is a brace passing the knot and keeps every stretch (cutting its first stretch lost
  4 to 7 mm braces on 13 real models).
- **Late sleeves.** A free piece lying along the rods of the one support it touches (within 0.15 mm),
  holding no contact of its own: a coupling round a joint, a doubled stretch.
- **Loose and floating.** Anything else, reported as an exception with its reason (for example a
  piece joined only to braces, which DragonFruit braces cannot host).
- **Roles.** Along each route: tip, trunk, branch or strut. A strut (a route ending on a footing) is
  a micro support when its median width is at most `MICRO_D`; so is a thin blunt rod from the model
  onto a pillar. A strut's piece holding its lower contact is a tip too (h01 to h04).
- **Balls.** A ball the model cuts at a routed contact is part of that tip (its loop sizes the tip,
  and it is never a joint: only tips enter the model; cball cases, p18). A ball between routed rods
  or braces is a knot. Any other ball is floating.

| Constant | Value | Why |
| --- | --- | --- |
| `MICRO_D` | 0.6 mm | A strut no wider than this (median) is a micro support, which may enter the model. |
| `STUB_PAST_MM` | 0.05 mm | A base stub runs at least this far past the end of the rod whose line it lies on. |
| `KNEE_DEG` | 80 degrees | A leg turns by at most this at a knee (Lychee kickstands lean 45 degrees off their pillar). |
| `KNOT_REACH_MM` | 2.0 mm | A run between a branch and its parent with both ends this close to the branch's knot is the branch's base (seen: 0.7 to 1.6 mm long; rods are about 1 mm wide, so a brace this near the knot would be solid with it). |
| `PAD_REACH_MM` | 1.0 mm | A pillar meeting a plate support's foot at most this far above the root top is its root (the reach of what stands on a pad). |
| `PAD_START_MM` | 0.6 mm | The base of a plate support's last rod ending at most this far above the root top is its root (where a piece standing on a pad may start). |

## Shaping

`classify/tips.rs` shapes each tip and measures the widths.

- **Socket.** The socket is where the first cone along the route that widens away from the contact
  (by at least 15%, starting within 2 mm) meets its shaft. A cone at the contact narrowing away from
  it is a blunt wide end (a micro rod's or strut's lower end), not a tip cone.
- **Contact width.** At the loop centroid. The written contact width is the loop's equal-area width
  divided by the square root of `DRAWN_AREA_SHARE` (DragonFruit's polygon allowance), clipped to
  `MIN_CD` to `MAX_CD`. A cone tip's contact is capped at its body width. A ball end in the model is
  as wide as its ball; a loop round several welded rods' ends is as wide as the widest of them
  (welded cases, real model 2c3b5fb1).
- **Blunt rod end.** A rod with no cone, ending at a slant so its loop is wider than the rod, keeps
  the loop's width as contact and the rod's width as body; the writer sinks it. Its socket is
  `BLUNT_MM` down the rod (an unread piece keeps 1 mm). Cases n01 to n03.
- **Stub.** A cone ending where its route ends (a tip straight onto another support's rod) keeps a
  stub of shaft to hang from the knot: at most `STUB_MM`, and at most a tenth of the route.
- **Struts.** A strut too short for two full tips gives each tip its share (0.45 of the length).
- **Sphere-tip data.** When a separate ball wider than the loop holds the contact (its centre within
  its radius plus 0.05 mm), the tip gets the ball's width, the cone's width at the ball's centre and
  the ball's centre. A rounded rod end (end cap radius under 0.35 R, the next rod section within 20%
  of R, the cap within R + 0.3 mm of the contact, the loop no wider than the rod) gets the same as a
  sphere tip of the rod's width.
- **Ball seats** (`ball_seats`). A sphere tip is seated at the original ball's own depth below the
  surface (`ball_depth`), so its sunk part is rebuilt, unless a ball seated there would cross the
  model a second time; then the writer keeps DragonFruit's own seat.
- **Shaft width** (`shaft_d`). The length-weighted median width of what the route walks past the tip:
  rods at their width, long tapers at the width of a rod of the same volume.
- **Root width** (`root_d`). The width whose DragonFruit root (a 0.5 mm disk under a 1.5 mm cone to
  the shaft at the 2 mm root top) best overlaps the original foot above the raft (the root pieces and
  the foot's own flare, taken as round), by intersection over union, never narrower than the shaft.
- **Strut kind** (`strut_kind`). A strut, or a micro support between two parts of the model, whose
  two end pieces both carry a cone is a stick; one where neither does is a twig, whatever its span
  (owner: h04_short, h05_flat_strut, m01 and m02). When both ends lie on one piece (a micro support's
  single rod), each end is judged by the section at that end.

| Constant | Value | Why |
| --- | --- | --- |
| `MIN_CD`, `MAX_CD` | 0.05 mm, 3.0 mm | The narrowest and widest contacts written (DragonFruit's minimum contact is 0.05 mm). |
| `DRAWN_AREA_SHARE` | 0.955 | The share of a circle's area DragonFruit's polygon contact draws; the written width is the loop's divided by its square root. |
| `BLUNT_MM` | 0.35 mm | A blunt tip's length. It is arbitrary; a short one keeps the shaft on the rod. |
| `STUB_MM` | 0.2 mm | Shaft kept under a cone whose route ends where it does, to hang from the knot. |

## Layout

`classify/layout.rs` turns routes, posts, legs and braces into the layout.

- **Lines.** Each support's line is simplified: bends are kept only where the line leaves the
  straight run by more than `STRAIGHT_MM`. A plate support's shaft starts at the root top; what the
  route does below it is the root.
- **Foot pillar** (`foot_pillar`). A plate support whose foot lies inside an upright root piece of
  its own (within `FOOT_PILLAR_COS` of vertical, reaching the root top, at most `FOOT_PILLAR_WIDTH`
  times the shaft's width) stands on that pillar's line, and the points of its own line inside the
  pillar are dropped. A tip cone whose wide end sits in a short pillar's top comes down inside the
  pillar off its line, which put the foot 0.35 to 0.54 mm from the real pillar
  (p05_tip_grazing, t02_trunk_wall_raft).
- **Socket clearance.** DragonFruit puts a joint ball (shaft width + 0.1 mm) on the socket, so a
  socket nearer the surface than that ball's radius pokes the ball into the model. Where the shaft
  leaves the tip within 30 degrees of the surface normal (`SOCKET_SQUARE_COS`), the socket moves down
  the route in 0.05 mm steps until the ball clears the surface by `SOCKET_CLEAR_MM`, by at most
  `SOCKET_MOVE_MM` (c03_shorttip). A shaft at a slant clears the surface with its own side, as the
  original's does. A longer move made real model fd7a5cf0 clip elsewhere.
- **Struts and micro supports between two parts of the model.** Both end tips with cones: written
  as a stick. Both flat: a twig. Otherwise (one end of each) DragonFruit's span rule (a twig under
  `STICK_MIN_SPAN_MM`). Micro supports are listed in the layout's `micro` set; the writer turns a
  short cone-ended one into a twig (below).
- **Braces.** A brace end at a branch's own knot that lies on the parent's line (within the parent's
  radius, within 1 mm of the knot) goes on the parent, never putting both ends on one support
  (l03_lean_ball). A brace with both ends on one support, or an end on a support that was not
  written, is an exception.
- **Posts and legs.** A post wholly under the root top is not written (DragonFruit's root replaces
  it). A leg under the root top, or leaning on a support that was not written, is an exception.
- **Stepped tapers** (`step_widths`, owner). DragonFruit tapers only twigs and a kickstand's terminal
  segment, so a strongly tapered trunk, branch, post or leg is written as up to three constant-width
  segments (`seg_d`, bottom to top), each step one more joint. Steps are earned only on a cone section
  of the route at least `STEP_MIN_MM` long on the written shaft. A second width needs an ideal
  overlap gain of `STEP_GAIN2` (about 1.47:1 for a linear taper), a third `STEP_GAIN3` more (about
  1.9:1). Cuts are least squares on the whole shaft's area profile, sampled every `STEP_SAMPLE_MM`;
  a cut within `STEP_SNAP_MM` of an existing joint or knot moves onto it. Legs step too: DragonFruit
  draws the kickstand's terminal taper but does not slice it. Cases t07, t08, t10 to t15.
- **Narrowing** (`narrow_widths`, owner). DragonFruit builds round shafts, so a polygon shaft whose
  flat runs close to a model face builds into it by the polygon's sagitta. A segment wider than
  `NARROW_MIN_D` (plus 0.01 mm) whose round build would come within `NARROW_CLEAR_MM` of the model is
  written narrower, just enough to clear it, by at most `NARROW_MAX_MM`, and only where the round
  build reaches no more than `NARROW_INSIDE_MM` into the model. The whole segment narrows (narrowing
  only the near stretch would need a step, one more joint). The last radius plus 0.3 mm under a
  socket and everything below the root top are not measured. Case c09_flatwall; no real model in
  the test corpus narrows. Shafts under 1 mm are not narrowed (tests give them more distance).
- **Surface normal.** Each contact's normal is the area-weighted normal of the model faces near it
  that face the support's socket side. A support reaching into an open body touches the back of its
  faces; then those faces are used, turned toward the support (n02).
- **Conversion.** `to_support_layout` writes the writer's `SupportLayout`: supports (a parent always
  before its branches), posts, braces and kickstands, with the optional `segment_diameters`,
  `kickstand_segment_diameters`, `sticks` and `twigs`. `to_json` writes the same layout in the
  prototype's JSON shape, which the synthetic suite reads.

| Constant | Value | Why |
| --- | --- | --- |
| `STRAIGHT_MM` | 0.12 mm | Bends of a line are kept only where it leaves the straight run by more than this (fewer joints). |
| `FOOT_PILLAR_COS` | cos 10 degrees | The pillar a plate support stands on is upright within 10 degrees ... |
| `FOOT_PILLAR_WIDTH` | 1.5 | ... and at most this many times as wide as the shaft (a pad or a foot cone is wider). |
| `SOCKET_CLEAR_MM` | 0.05 mm | The socket's joint ball clears the model's surface by this much ... |
| `SOCKET_SQUARE_COS` | cos 30 degrees | ... where the shaft leaves the tip within 30 degrees of the normal ... |
| `SOCKET_MOVE_MM` | 0.3 mm | ... where moving the socket by at most this clears it (a longer move made a real tip clip elsewhere). |
| `STEP_GAIN2` | 0.08 | A second width needs this gain in ideal overlap (about 1.47:1); owner confirmed. |
| `STEP_GAIN3` | 0.045 | A third width needs this much more (about 1.9:1); owner confirmed. |
| `STEP_MIN_MM` | 4.0 mm | Only a taper section at least this long on the written shaft earns steps. |
| `STEP_SNAP_MM` | 1.5 mm | A cut within this of a joint or knot moves onto it (no extra joint). |
| `STEP_SAMPLE_MM` | 0.2 mm | The shaft radius is sampled this often for the cuts. |
| `NARROW_MIN_D` | 1.0 mm | Only shafts thicker than this are narrowed (owner). |
| `NARROW_CLEAR_MM` | 0.02 mm | A shaft whose round build comes within this of the model is narrowed to clear it by this. |
| `NARROW_MAX_MM` | 0.14 mm | Narrowing is at most this, inside the suite's 0.15 mm width tolerance. |
| `NARROW_INSIDE_MM` | 0.08 mm | Only where the round build reaches at most this far into the model: a polygon's flat beside it (the sagitta of a 12-sided 3 mm shaft is 0.051 mm, of an 8-sided 1.5 mm one 0.057 mm). |

## Writing the VOXL

`voxl.rs` writes the scene: a header, a chunk directory and zlib chunks, with the model mesh and its
supports as DragonFruit roots, trunks, branches, twigs, sticks, braces, kickstands and knots. Posts
are trunks without a contact cone.

- **Per-segment widths.** `segment_diameters` (supports and posts) and
  `kickstand_segment_diameters` give each segment its own width. A joint is as wide as the wider
  segment it joins plus `JOINT_DIAMETER_OFFSET_MM`. A knot on a host takes the width of the host
  segment it sits on.
- **Strut type** (`strut_ends`). The `sticks` and `twigs` sets force the strut type whatever the span;
  other struts follow DragonFruit's own rule (a twig under `STICK_MIN_SPAN_MM`). A micro support in
  the `sticks` set that is shorter than its two contact cones as DragonFruit builds them (each from
  the contact to the far side of its socket joint, the tip's length plus half the shaft width plus
  `JOINT_DIAMETER_OFFSET_MM`) is written as a twig all the same: as a stick its cones and socket
  joints overlap and build about twice its width at the contacts (owner, 2026-11-11).
- **Twig disks.** DragonFruit builds each twig contact disk along the surface normal and caps it with
  a ball of the disk's radius; the shaft runs between joints at the disk tops. A micro support's
  disks stand off by that ball's radius (`MICRO_TWIG_STANDOFF_SHARE`, half the shaft width), so the
  ball stays out of the model and a support of a few tenths of a millimetre does not bend into a kink.
  Other twigs keep the longer disk (the joint's radius plus clearance, at least the disk thickness):
  on Eastman's twigs the short disk sank the wider end joints into the model (8e6618e3: 5.9 mm2 more
  clip).
- **Contact placement** (`sliced_contact_placement`). The slicer runs the contact cone on into the
  model by the tip offset along the surface normal and leaves its flat cap square to the cone's axis,
  so a leaning cone meets the surface in an ellipse whose centre moves along the lean, cut short once
  the cap leans out of the surface. The written contact width is solved so the sliced loop has the
  contact's area, and the position moved back against the lean. Past a 75 degree lean the cone runs
  along the surface and the straight tip's width is written.
- **Blunt rod ends** (`sunk_blunt_placement`). Where the contact is wider than the body, the cone
  keeps the rod's width and is sunk along the surface normal until the sliced loop has the contact's
  area, at most the whole cap (as the original rod's end is). Twig disks stay at the shaft width.
- **Sphere tips.** Behind `SPHERE_TIPS_ENABLED`, off. When on, a tip with a ball is written as a
  DragonFruit sphere tip seated at the original's depth (`sphereDepthMm`), the cone from the
  original cone's width at the ball's centre, or at DragonFruit's own seat where the reader found no
  depth. The round-trip tool can turn it on (`SPHERE_TIPS=1`).
- **Brace knots.** DragonFruit marks down a brace knot near either end of its segment and may move it
  to another segment, so brace knots are kept `BRACE_KNOT_END_SHARE` inside their segment and
  flagged as imported brace ends so DragonFruit keeps their width.

| Constant | Value | Why |
| --- | --- | --- |
| `JOINT_DIAMETER_OFFSET_MM` | 0.1 mm | DragonFruit draws joints this much wider than their shaft. |
| `NOMINAL_TIP_OFFSET_MM` | 0.05 mm | The slicer's default tip offset (one 0.05 mm layer, no 3D anti-aliasing); the contact placement is solved for it. `penetrationMm` plays no part in slicing. |
| `MIN_WRITTEN_CONTACT_MM` | 0.05 mm | Narrowest contact DragonFruit accepts. |
| `TIP_PENETRATION_MM`, `TIP_DISK_THICKNESS_MM`, `TIP_MAX_STANDOFF_MM` | 0.1, 0.1, 0.25 mm | DragonFruit's own contact tip defaults. |
| `STICK_MIN_SPAN_MM` | 5.0 mm | DragonFruit's span rule, for struts with one cone end and one flat end: closer than this is a twig, else a stick. |
| `TWIG_JOINT_RATIO`, `TWIG_JOINT_CLEARANCE_MM` | 1.1, 0.05 mm | A twig's end joints are this much wider than its disks and stand off by their radius plus this. |
| `MICRO_TWIG_STANDOFF_SHARE` | 0.5 | A micro support's twig disks stand off by half the shaft width (the radius of DragonFruit's disk ball). |
| `BRACE_KNOT_END_SHARE` | 0.025 | DragonFruit marks down a brace knot within 0.02 of either end of its segment; knots are kept just inside that. |
| `COORDINATE_DECIMALS` | 4 | Support coordinates are rounded to 0.1 micron. |
| `SPHERE_TIPS_ENABLED` | false | Sphere tips need DragonFruit's `sphereDepthMm`, not released yet; released DragonFruit builds sphere tips with undersized contacts (owner: off in the app). |

## Exceptions

The classification lists what it could not place, with a reason, in `Classification::exceptions`:
contacts with no piece, no route, or passed through by a routed support; pieces left loose or
floating; and parts the layout could not write (a post or leg under the root top, a leg on an
unwritten support, a brace with both ends on one support). `Classification::check` counts pieces,
unread pieces, routed, unrouted and passed-through contacts, routes found only with near misses or
going up, sleeves, width steps, narrowed segments, ball seats and the written parts.

## Measures

The reader is judged on the original supports against what DragonFruit builds from the export, never
against its own labels (earlier measures flattered the reader).

- **Support volume overlap** (headline). Rays along each axis on a jittered 0.025 mm lattice give
  exact solid intervals of the original supports A and the DragonFruit build B. Coverage is the
  volume in both over A (how much of the original was rebuilt), fit is both over B (how much of the
  build is where the original was), and IoU is both over the union. The raft band is excluded, and
  the overlap outside the model is the one a print keeps; the tool reports both with and without the
  part inside the model. Pooled values sum the volumes over models.
- **Missing and extra tips.** The original's touch-rule contacts are paired one to one with built tips
  whose loop centre at the 0.05 mm offset is within 0.5 mm and within half the two widths. Missing
  are contacts with no built tip; extra are built tips with no contact (a leftover tip whose loop
  centre lies inside a contact's loop is not extra).
- **Tip size.** The matched tip's loop on a DragonFruit build at its own 0.05 mm tip offset, as
  built, within 10% and within 20% of the original contact's equal-area width.
- **Clipping.** The area (mm2) of the build's patches inside the model that are not tips.
- **Lychee project truth.** For models that come with a Lychee project (`.lys`), the project's own
  supports are the truth: tips found, missing and extra, position, size and direction of tips,
  shaft recall and precision, routes right (within 0.5 and 1.0 mm), braces (ends within 1 mm) and
  joints per support. The Lychee readers and scorer are kept with the support work files outside
  the repository.

## Checking the reader

- **Synthetic suite.** `cargo test -p findamodel --test support_synthetic` generates 122 small cases
  in 1075 variants (8, 12, 16 and 32 sides, the scene turned, joined and welded shells), each with
  exact truth, reads them and checks: validate (the truth against its own mesh), 1 reader labels, 2
  layout, and 4a clips and 4b loops on a nominal build (the layout meshed roughly as DragonFruit
  builds what the VOXL writer writes, with the writer's own tip placement and strut ends,
  `support_synthetic/nominal.rs`). Every variant's stage results are held to
  `support_synthetic/expected.json`; a stage that starts or stops passing fails the test
  (`SYNTHETIC_UPDATE=1` rewrites the file). Tolerances are the prototype's, unchanged. See
  [Support reader test cases](support-reader-tests.md) for every case and its result;
  `scripts/synthetic-catalogue.py` regenerates that page.
- **Real-structure suite.** `cargo test -p findamodel --test support_real` reads support structures
  cut out of real pre-supported models whose Lychee project is known (`backend/tests/support_real/`:
  per case the original support shells, small closed cuts of the model where the supports meet it,
  the Lychee truth and a picture of the expected result) and checks the truth against its own
  meshes (validate), then tips, tip shape, routes, shafts, braces and joints against the Lychee
  truth, and 4a clips and 4b loops on the nominal build. The stage results are held to
  `support_real/expected.json` in the same way (`REAL_UPDATE=1` rewrites it). See
  [Support reader real-structure test cases](support-reader-real-tests.md) for every case, its
  pictures and its result; `scripts/real-support-cases.py` cuts the cases from the case list
  `support_real/cases.json`, and `scripts/real-support-catalogue.py` regenerates the page.
- **DragonFruit round trip.** Stage 3 (build recall and precision) and stage 4 on DragonFruit's own
  build, and every measure above on real models, come from the round trip:
  `backend/tools/support-reader` (see its `README.md`) and `scripts/support-roundtrip.py`.
- When changing a rule, run the synthetic suite and then the round trip on a few small real models
  that show the pattern, and compare overlap and tip measures before and after.

## Known limits and open points

- **Sphere tips are off.** `SPHERE_TIPS_ENABLED` stays false until DragonFruit's `sphereDepthMm`
  support is released. Until then ball tips are written as cones sized from the loop, and rounded rod
  ends as blunt ends.
- **Tip depth is fixed at 0.05 mm.** Contacts are sized for DragonFruit's default 0.05 mm tip offset;
  a per-tip depth is a TASKS.md item.
- **Prototype nondeterminism.** The port is deterministic, but the prototype was not in three places
  found while porting, so those cases cannot be compared exactly with prototype runs: the prototype
  touch tool's hash order on n03_endin_nonman (the port gives one of its outcomes); the prototype
  pieces tool's segment order and welded-union axis on f03_two (16 sides, welded); and the same
  segment order on real model b0f32f5b (post and leg roles of mirror pieces swap between prototype
  runs).
  The port's reading is the same on every run and on any number of threads (checked on all 1075
  variants at 1, 4 and 40 threads and on real models; the synthetic suite's
  `the_reader_is_deterministic_across_runs_and_thread_counts` holds it on those cases). The round
  trip's `touch` measure walks its maps in key order as the reader does, so it no longer varies
  between runs either (k03_modeltree's clip was 2.03 or 2.45 mm2 and is now always 2.45, the value
  where the shaft on the y side clips as its twin on the x side does). On n03_endin_nonman the measure's
  contacts now equal the reader's; there the body-top loop under the tip is judged by a vote of
  three of its segments, and the stray sheet's winding fails the vote on one side of the sheet, so
  the answer depends on where the loop's chain starts (fixed now, but not chosen on merit).
- **Nominal build.** Stage 4 in cargo runs on an approximation of DragonFruit's build; leaning tip
  placement differs, so only the round trip is final on stage 4.
- **Unread pieces** are used as straight stretches at their width; a bent or welded solid that
  neither split finds keeps 1 mm blunt tips.
