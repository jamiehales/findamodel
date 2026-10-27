//! Rebuilds the layout of a pre-supported model's supports (contact tips, shaft paths,
//! branch points and roots) from their geometry, so a slicer can regenerate them.
//!
//! The support triangles are sliced into thin horizontal layers. Each closed outline in a
//! layer is a blob, so separate shells stay apart even where they overlap (crowded or crossing
//! shafts), while an outline lying inside another (a shaft inside its joint ball) joins it.
//! Blobs in neighbouring layers that overlap are linked. That graph follows every shaft
//! whether the supports are separate primitives or one unioned mesh. A blob with nothing above it that touches the model is a contact tip; the contact
//! itself is where the shaft leaves the model, since tips are buried a little way into it (some
//! end in a ball inside it).
//! Walking down from the tip, at each fork taking the most vertical way (a brace leaves its
//! post at an angle) and then the one that best continues the shaft, traces the shaft to the
//! plate (a trunk) or into a shaft an earlier walk claimed (a branch). A way beside the shaft
//! rather than below it (another support whose slices touch the shaft's) is passed up, unless
//! that loses the support. A leaning shaft keeps to its own slices past an upright post it
//! converges on: the post is the more vertical way, but off to the side of where the shaft is
//! heading. Tips are walked from the highest down, so the tallest support of a tree is its
//! trunk. Blobs much wider than the shaft, or off its line (raft, braces, crossing shafts), are
//! passed through without bending the shaft's centre line or ending the walk.
//! Where a post the walk is following merges with a brace or rung, the blob's middle lies off
//! the post, so the walk keeps to the post's own line through it (struts tied by rungs inside a
//! hollowed model). Below a tip leaning steeply away from its contact, or past a bend, the walk
//! takes the shaft up again as soon as the slices it passed settle into a straight run of one
//! width, from where the run began. Joint balls and junctions, wider than the slices around them,
//! are left out of the shaft's line and width.
//!
//! A walk ending in the model, rather than on the plate or a shaft, is a strut between two parts
//! of the model. Tips meeting the model at one place are one tip with two tops when their walks
//! run together, and two supports when they keep apart for a while (struts meeting in pairs at
//! one contact inside a hollowed model, a fan of tip cones running into one joint). A strut
//! whose walk runs into a strut packed beside it, where their feet or a rung merge them, keeps to
//! its own foot when it can reach the model without the other.
//!
//! Many support exports model each tip as a cone of its own, a closed shell apart from the
//! joint ball and shaft it overlaps. Such a tip is measured from the cone itself (its narrow
//! end is the contact size, its wide end the socket), which slices of a leaning tip only
//! roughly show; so are both tips of a strut. Other tips are fitted to the walk's slices.
//!
//! Support geometry no walk passed is left in connected pieces. A piece bridging two traced
//! supports is a brace; where the two braces of an X meet each support in one junction, the
//! junction's lower and upper parts are their ends. A piece standing on the plate and leaning
//! against a traced support (the legs splaying from a tree's trunk) is a kickstand. So is a post
//! standing on the plate with a strut leaning off it onto a traced support, from its top or
//! lower down, or running straight up into one: DragonFruit's own kickstand is a column with a
//! strut leaning off its top. Each post holds up one kickstand, and what else meets it is
//! braced to that kickstand. A kickstand cannot hold up another, so a post that legs lean
//! against, splaying from it down to the plate, stands as a post of its own (a DragonFruit trunk
//! without a contact cone), which its legs hold up as kickstands and its struts brace to traced
//! supports; so does a post standing free beside traced supports it is only braced to. A post's
//! column carries on past junctions a traced support's walk passed through.
//!
//! Each tuned value below says what shape it handles, which support styles rely on it (the
//! groups of `docs/support-styles.md`; "all" when they do not differ) and the data behind it.
//! The data comes from the heuristics audit, which nudged every value 25% either way over the
//! DragonFruit round trip (recall and precision of the rebuilt supports against the originals at
//! 0.1 and 0.3 mm) and the guard sets. Load-bearing: a nudge moves the fit by 0.0005 or more.
//! Fragile: a nudge flips a model in or out of the guard, or costs the fit 0.002 or more. No
//! effect: layouts change but the fit moves less than that, or nothing changes. A safety limit
//! only binds on shapes outside the corpus. Values retuned in the audit give the old value and
//! why it moved.
//!
//! Coordinates are Z-up millimetres with the plate at Z = 0.

use std::cell::Cell;

use rayon::prelude::*;
use rustc_hash::FxHashMap;

use super::{MIN_FACE_TILT, Triangle, Vec3};

/// Slice spacing; coarser when the supports are too tall for [`MAX_LAYERS`]. Styles: all. Sweep:
/// fragile. It is both the resolution and a guard input: 0.075 keeps an Engin3d fake (625a295a) and
/// takes 31% longer; 0.133 costs precision 0.0015 at 0.3 mm and BS Sculpt's 630a133f 0.13 recall.
const LAYER_MM: f32 = 0.1;
/// Safety limit on cost: taller supports are sliced coarser.
const MAX_LAYERS: usize = 4_000;
/// Layers sliced together on one thread. Threading only: no effect on any result.
const LAYERS_PER_BAND: usize = 32;
/// Cut points closer than this are one point: neighbouring triangles cut a shared edge at the same
/// place, which is what joins their segments into one outline. A numeric tolerance.
const OUTLINE_SNAP_MM: f32 = 0.0005;
/// Directions a cross-section's width is measured across, every 5 degrees: a slanted shaft's
/// elliptical slice then reads within about 1.5% of its diameter up to a 75 degree lean. Styles:
/// all. Sweep: no effect (27 or 48 directions move the round trip under 0.0005).
const WIDTH_DIRECTIONS: usize = 36;
/// Grid cell for blob containment and layer-to-layer overlap lookups. A lookup grid: no effect on
/// any result.
const BLOB_CELL_MM: f32 = 1.0;
/// Blobs in neighbouring layers whose boxes come this close are linked, so a steeply leaning
/// shaft's slices stay one shaft. Styles: all. Sweep: no effect (25% either way moves the round
/// trip under 0.001).
const LINK_MARGIN_MM: f32 = 0.05;
/// Grid cell for nearest-surface lookups on the model. A lookup grid: no effect on any result.
const SURFACE_CELL_MM: f32 = 1.0;
/// A blob top within this distance of the model surface is a contact tip, and a cone shell is its
/// tip when the cone's narrow end is this close to the contact. Styles: all. Sweep: no effect (both
/// uses, measured apart before they were merged).
const CONTACT_MAX_MM: f32 = 1.0;
/// A blob top inside the model within this distance of its surface is a contact tip too: some
/// exports end a tip in a ball buried in the model. Styles: all. Sweep: no effect.
const BURIED_CONTACT_MAX_MM: f32 = 2.5;
/// How far from the shaft the model surface is searched for where the tip enters it: as deep as a
/// buried tip reaches.
const ENTRY_SEARCH_MM: f32 = BURIED_CONTACT_MAX_MM;
/// A blob wider than `width * WIDEN_RATIO + WIDEN_SLACK_MM` of the shaft being walked is a raft,
/// brace or crossing: walked through, but neither a shaft point nor a branch point. Styles: all,
/// most on rafts and the lattices. Sweep: no effect (ratio and slack each).
const WIDEN_RATIO: f32 = 1.5;
/// See [`WIDEN_RATIO`]: the slack keeps the test from binding on the thinnest tips.
const WIDEN_SLACK_MM: f32 = 0.25;
/// A fork blob wider than `width * MERGED_RATIO + MERGED_SLACK_MM` of the shaft the walk follows is
/// that shaft merged with a brace or crossing bar beside it, and so is a slice that much wider than
/// a post's own. Styles: the lattices and zigzag webs, hollow-struts (rungs). Sweep: no effect
/// (ratio and slack each).
const MERGED_RATIO: f32 = 1.25;
/// See [`MERGED_RATIO`].
const MERGED_SLACK_MM: f32 = 0.05;
/// A slice no wider than `width * SHAFT_ALONE_RATIO + SHAFT_ALONE_SLACK_MM` is the shaft of that
/// width on its own. On a shaft the walk is following, a wider one is a joint ball, or the shaft
/// merged with a brace or crossing bar: a shaft point only when centred on the shaft. A shaft blob
/// wider than that around the median of those within `LOCAL_WIDTH_MM` of its height is left out of
/// the shaft's line, and a run of skipped slices keeps one width within the same tolerance either
/// way. Styles: all. One tolerance since the audit merged three (1.1 with slack 0.05 or 0.03, and
/// 0.1 with 0.03 for runs): over the 139-model corpus recall and precision moved under 0.0005.
const SHAFT_ALONE_RATIO: f32 = 1.075;
/// See [`SHAFT_ALONE_RATIO`].
const SHAFT_ALONE_SLACK_MM: f32 = 0.04;
/// How quickly the walked shaft width follows the blobs it passes. Styles: all. Sweep: no effect.
const WIDTH_SMOOTHING: f32 = 0.25;
/// The walked shaft width grows by at most this much per mm walked: enough to follow a widening
/// tip, too little to follow a brace peeling off the shaft (which widens the blob a little each
/// layer). Styles: all. Sweep: no effect on the fit (0.2 matches 30 more tips but costs 0.0003).
const MAX_WIDTH_GROWTH: f32 = 0.15;
/// A blob whose centre is further than `DRIFT_MM + width * DRIFT_RATIO` from where the shaft is
/// heading is a brace merging into the shaft, so it is not a shaft point either. A slice of a
/// column rising from a kickstand's foot (a post) is the post on its own when its middle lies
/// within `DRIFT_MM` of the post's centre line. Styles: all, most the lattices and zigzag webs.
/// Retuned in the audit from 0.1 and 0.3: a merging brace is caught sooner, recall and precision at
/// 0.1 mm up 0.004 over the corpus, 0.3 mm flat (Adament Arsenal's 70b02245 lost 0.05 recall at a
/// ratio of 0.225).
const DRIFT_MM: f32 = 0.075;
/// See [`DRIFT_MM`].
const DRIFT_RATIO: f32 = 0.15;
/// Drift is measured from the shaft as it was this far back, so a blob creeping sideways a little
/// each layer cannot drag the measure along with it. Styles: all. Sweep: no effect.
const DRIFT_LAG_MM: f32 = 0.5;
/// After this much height of skipped blobs narrower than [`MAX_RESET_WIDTH_MM`], the shaft really
/// is thicker than its tip or really does bend, so the walk takes the blob as is. Styles: the
/// lychee groups (long leaning tips). Sweep: load-bearing (2.0 costs recall 0.002 at 0.3 mm).
/// Lychee zigzag webs do best at 2.0 (14% fewer joints there), but it cuts new pieces into the
/// model on Eastman's ef71a8ec and Mezgike's ec811c80; as a multiple of the shaft width, Lychee
/// trees and Eastman pulled opposite ways.
const SKIP_RESET_MM: f32 = 1.5;
/// See [`SKIP_RESET_MM`]: wider blobs are rafts or junctions, never the shaft (a half-width).
/// Sweep: no effect.
const MAX_RESET_WIDTH_MM: f32 = 2.0;
/// A run of skipped blobs this tall that keep one width and one step from slice to slice is a shaft
/// (below a leaning tip, or past a bend): the walk picks it up from where the run began and steers
/// by its lean until it has come far enough along it to measure its own direction. The run's slices
/// keep within [`SLOPE_TOLERANCE`] of the line its first slices set, and their widths within
/// [`SHAFT_ALONE_RATIO`] and [`SHAFT_ALONE_SLACK_MM`] of its first slice's. Styles: the lychee
/// groups. Sweep: no effect; as a multiple of the shaft width it traded recall for precision.
const PICKUP_RUN_MM: f32 = 0.6;
/// A run leans no more than this (horizontal run per unit of height, about 56 degrees). Styles: the
/// lychee groups. Sweep: no effect.
const MAX_RUN_LEAN: f32 = 1.5;
/// A run of at least this many skipped slices, carried on by the slice where the walk gives up
/// skipping (see [`SKIP_RESET_MM`]), is the shaft. Styles: the lychee groups. Sweep: no effect.
const RESET_RUN_SLICES: usize = 3;
/// A shaft whose course leans at least this much (horizontal run per unit of height) keeps to its
/// own slices past an upright post it touches. Styles: lychee-lattice (leaning shafts past cage
/// posts). Sweep: no effect.
const MIN_COURSE_LEAN: f32 = 0.15;
/// Height over which the shaft direction is measured: long enough that one slice's wandering middle
/// barely turns it, short enough to know a tip's direction soon. Styles: all. Sweep: load-bearing
/// (0.75 costs 0.0006, 1.33 matches 17 fewer tips); the same in every style.
const DIRECTION_SPAN_MM: f32 = 1.0;
/// Where the structure forks going down, each way is followed this far and the most vertical one
/// taken: a brace leaves the post it hangs from at an angle, and a bent shaft can point straight at
/// it. Styles: all, most the lattices. Sweep: no effect. As a multiple of the way's width it cost
/// Eastman's 8e6618e3 0.023 recall, and with a 1.5 mm floor kept two Engin3d fakes.
const FORK_LOOKAHEAD_MM: f32 = 2.0;
/// The ways followed down from a fork, at each slice those nearest the line straight down. A limit
/// on cost: no effect between 6 and 11, and without it the walks through DM Stash's dense lattice
/// (354e5d8b) take 2.6 times as long.
const FORK_WAYS: usize = 8;
/// Two leans (horizontal run per unit of height) within this of each other are alike: ways down
/// from a fork whose slopes differ by less are told apart by the shaft's direction, and a run of
/// skipped slices keeps within this of the line its first slices set. Styles: all. Sweep:
/// load-bearing for forks (0.1125 matches 22 fewer tips; 0.2 gains 0.0003, and adds 2.7% joints as
/// the run tolerance it replaced). Chitubox X lattices would take 0.075 and Lychee lattices 0.3, at
/// 17% more joints.
const SLOPE_TOLERANCE: f32 = 0.15;
/// The tip's socket is looked for within this distance of the contact. Styles: all. Sweep: no
/// effect.
const TIP_WINDOW_MM: f32 = 4.0;
/// The shortest tip, from contact to socket. Styles: all. Sweep: no effect. As a multiple of the
/// tip's width it cost Lychee lattices 0.003 recall.
const MIN_TIP_LENGTH_MM: f32 = 0.6;
/// The socket is the first point reaching this fraction of the tip window's widest blob (a joint
/// ball), or where the tip stops widening (see [`PLATEAU_GROWTH_RATIO`]). Styles: all, most Lychee
/// zigzag webs. Sweep: load-bearing (0.8 matches 11 fewer tips). Retuned in the audit from 0.85:
/// over the 158 round-trip and benchmark models 0.95 fits as well in every style, with 4% fewer
/// joints per support (9% on Lychee zigzag webs).
const TIP_BODY_FRACTION: f32 = 0.95;
/// The tip stops widening where it widens by less than this share of the tip window's widest slice
/// over the next [`PLATEAU_SPAN_RATIO`] of it: a thicker tip's cone widens more over a longer run.
/// Styles: all. Was a fixed 0.03 mm over 0.3 mm, which the ratios keep at the corpus median (1.16
/// mm); no measurable effect on the corpus.
const PLATEAU_GROWTH_RATIO: f32 = 0.025;
/// Floor of [`PLATEAU_GROWTH_RATIO`], half the old fixed value.
const PLATEAU_GROWTH_MIN_MM: f32 = 0.015;
/// See [`PLATEAU_GROWTH_RATIO`].
const PLATEAU_SPAN_RATIO: f32 = 0.25;
/// Floor of [`PLATEAU_SPAN_RATIO`], half the old fixed value.
const PLATEAU_SPAN_MIN_MM: f32 = 0.15;
/// A tip may take up this share of its support's length. Styles: all. Sweep: load-bearing (0.45
/// costs 0.0006). Lychee lattices would take 0.9, partly because their tips are long.
const TIP_SHARE: f32 = 0.6;
/// A strut's two tips share one shaft, so each takes less. Styles: hollow-struts. Sweep: no effect.
const STRUT_TIP_SHARE: f32 = 0.4;
/// A tip's narrow end, which is usually buried a little way in the model, is the top slice that is
/// at least this fraction of the slice below it (narrower top slices cut through an end cap). The
/// contact is as wide as the tip where it leaves the model, and no narrower than that end. Styles:
/// all. Sweep: no effect.
const CAP_WIDTH_FRACTION: f32 = 0.8;
/// Safety limit: the narrowest contact rebuilt.
const MIN_CONTACT_DIAMETER_MM: f32 = 0.2;
/// Safety limit: the widest contact rebuilt.
const MAX_CONTACT_DIAMETER_MM: f32 = 1.5;
/// Safety limit: the thinnest rod rebuilt (a support's shaft, a brace or a post).
const MIN_ROD_DIAMETER_MM: f32 = 0.3;
/// The shaft diameter is this quantile (1/N) of its cross-sections above the root: a shaft keeps
/// one width, or tapers towards its foot, and the joint balls and merged braces left in only ever
/// widen a section. Styles: all. Sweep: load-bearing (the median, 2, costs precision 0.008 at 0.1
/// mm; 4 costs recall 0.004 at 0.3 mm); the same in every style.
const SHAFT_WIDTH_QUANTILE: usize = 3;
/// Bends smaller than this share of the shaft's diameter are dropped from the shaft path, and
/// always those smaller than `SIMPLIFY_MIN_MM`: a thick shaft's slices wander further off its line
/// without it bending. Styles: all. Was a fixed 0.3 mm, kept at the corpus median shaft (1.2 mm);
/// Eastman's 1.3 to 1.5 mm posts get 12% fewer joints for 0.001 recall at 0.3 mm. A smaller
/// tolerance fits closer (0.15 mm: recall and precision at 0.1 mm up 0.01) but more than doubles
/// the joints.
const SIMPLIFY_RATIO: f32 = 0.25;
/// Floor of [`SIMPLIFY_RATIO`], the old fixed value: thinner shafts keep no more joints than
/// before.
const SIMPLIFY_MIN_MM: f32 = 0.3;
/// A shaft blob much wider ([`SHAFT_ALONE_RATIO`]) than those within this distance of its height
/// (their median) is left out of the shaft's line. Styles: all. Sweep: no effect, also as a
/// multiple of the shaft width.
const LOCAL_WIDTH_MM: f32 = 0.5;
/// A contact this close to an earlier support's contact is a second top of that tip, unless the
/// walk from it keeps apart from that support until at least `OWN_SUPPORT_MIN_MM` from the contact:
/// then it is a support of its own meeting the model at the same place (struts meeting in pairs at
/// one contact inside a hollowed model, a fan of tip cones running into one joint). Pins and pegs
/// between model parts reach less. Styles: hollow-struts, fans. Sweep: no effect (each).
const DUPLICATE_CONTACT_MM: f32 = 0.5;
/// See [`DUPLICATE_CONTACT_MM`].
const OWN_SUPPORT_MIN_MM: f32 = 1.0;
/// A walk ending within this height of the lowest support layer stands on the plate, when that
/// layer is itself this close to the plate (Z = 0). Safety limit: supports exported on the plate
/// start within a slice of it, so this only binds for supports standing clear of the plate (a model
/// exported raised, or cut off below): their walks end stranded, so they are left out, with no
/// posts or kickstands.
const PLATE_REACH_MM: f32 = 0.5;
/// Brace detection: untraced pieces of at least this many cross-sections. Styles: the lattices and
/// zigzag webs. Sweep: no effect.
const MIN_BRACE_BLOBS: usize = 3;
/// The places a piece touches one support group into one brace end when they are within this much
/// height. Styles: the lattices and zigzag webs. Retuned in the audit from 1.0: two brace ends
/// close together on one post stay apart (recall at 0.3 mm up 0.002, precision down 0.0006; DM
/// Stash, Dakka Dakka, Bite the Bullet and Dakka Terrain models up 0.02).
const BRACE_END_GAP_MM: f32 = 0.75;
/// A brace is at least this many times its diameter long, and at least `MIN_BRACE_MIN_MM` (see
/// [`min_brace_mm`]). Styles: the lattices and zigzag webs. Sweep: fragile as a fixed 2 mm (2.67
/// lost common 2 to 2.7 mm braces, recall at 0.1 mm down 0.013). The thin braces of Chitubox X
/// lattices (0.6 to 0.8 mm) pack 1.5 to 2 mm apart and were lost below a fixed 2 mm, while braces
/// at the corpus median of 1.2 mm keep a minimum near it (1.8 mm). A ratio of 2.0 lost thick braces
/// (recall at 0.3 mm down 0.005 on the round-trip subset).
const MIN_BRACE_RATIO: f32 = 1.5;
/// Floor of [`MIN_BRACE_RATIO`]: a floor of 1.0 matched 1.5 on thin braces but cost precision on
/// Lychee lattices.
const MIN_BRACE_MIN_MM: f32 = 1.5;
/// A brace's piece covers at least this share of the line between its ends (leaving out the outer
/// `BRACE_END_SKIP` at each end, where it merges into the supports), within `BRACE_MARGIN_MM`:
/// which tells an X or a V from the lines its ends do not run along. Styles: the lattices and
/// zigzag webs. Sweep: load-bearing, trading recall against precision (0.7: recall at 0.3 mm up
/// 0.003, precision down 0.007). 0.9 and a margin of 0.075 each did better alone, but together lost
/// 16% of braces.
const BRACE_COVERAGE: f32 = 0.8;
/// See [`BRACE_COVERAGE`]. Sweep: load-bearing (0.2 costs precision 0.005 at 0.1 mm). Lychee zigzag
/// webs want 0.1125 (0.0016 better), which no size explains.
const BRACE_END_SKIP: f32 = 0.15;
/// See [`BRACE_COVERAGE`]: how far a slice's outline may lie from the line. Sweep: load-bearing,
/// trading recall against precision (0.1125: recall down 0.002, precision up 0.002).
const BRACE_MARGIN_MM: f32 = 0.15;
/// The line between a brace's ends is tested every this far. Sweep: no effect.
const BRACE_SAMPLE_MM: f32 = 0.2;
/// A brace's end is within this share of the brace's diameter of its shaft, beyond the shaft's
/// radius, for a knot to hang it there (the end is the middle of where the brace merges into the
/// shaft, which lies further out on a thicker brace), and always within `KNOT_REACH_MIN_MM`.
/// Styles: the lattices, zigzag webs and hollow-struts' rungs. Was a fixed 0.5 mm (no effect in the
/// sweep), kept at the corpus median brace (1.2 mm); thick braces keep their knots (116 more
/// braces, hollow-struts recall up 0.0005).
const KNOT_REACH_RATIO: f32 = 0.42;
/// Floor of [`KNOT_REACH_RATIO`], the old fixed value: a floor of 0.25 cost Chitubox X lattices
/// 0.004 recall.
const KNOT_REACH_MIN_MM: f32 = 0.5;
/// Where a piece touches each of two supports in one junction whose lower and upper parts lie at
/// least as far apart as the piece is thick, it may be an X of two braces meeting each support one
/// above the other; it is when the piece's cross-sections have their middles within this distance
/// of both braces' lines. Styles: chitubox-xlattice, lychee-lattice. Sweep: no effect.
const X_CENTRE_MM: f32 = 0.25;
/// Braces whose ends are this close to an earlier brace's are the same. Styles: the lattices and
/// zigzag webs. Sweep: no effect; sharing one value with [`DUPLICATE_CONTACT_MM`] was worse.
const DUPLICATE_BRACE_MM: f32 = 1.0;
/// A brace passing this close to a third support's end, when the piece also braces each of its ends
/// to that support, runs on through it: the two shorter braces replace it. A kickstand passing this
/// close to another support's shaft is likewise a shorter kickstand from that support, with a brace
/// running on in line with it, unless it passes within `THROUGH_ROOT_MM` of the root top, where it
/// only meets the support's root. Styles: the lattices and zigzag webs. Sweep: no effect (each).
const BRACE_THROUGH_MM: f32 = 0.8;
/// See [`BRACE_THROUGH_MM`].
const THROUGH_ROOT_MM: f32 = 1.0;
/// A brace whose middle third passes through the centre line of a third support's shaft runs
/// through that support: it is two braces meeting there, or none. Styles: the lattices. Sweep: no
/// effect.
const THROUGH_SKIP: f32 = 1.0 / 3.0;
/// A brace whose middle lies along a traced shaft, within this cosine of its direction, is a sliver
/// of that shaft rather than a brace. Styles: the lattices and zigzag webs. Sweep: no effect.
const BRACE_PARALLEL_COS: f32 = 0.9;
/// The ends are refitted to a line through the piece's own cross-sections when those spread along
/// at least this share of the brace (a level bar's slices do not, being one long slice). Styles:
/// the lattices, zigzag webs and hollow-struts. Sweep: fragile at 0.5 (0.67 cost precision 0.004 at
/// 0.3 mm). Retuned in the audit from 0.5: a quarter fits as well or better in every style over the
/// 158 round-trip and benchmark models, most on Lychee zigzag webs and hollow-model struts, whose
/// junctions hide much of each brace.
const BRACE_REFIT_SPREAD: f32 = 0.25;
/// Safety limit: the thickest brace rebuilt. Without it, thick bars of two Engin3d kits without
/// supports (ede32301, f5ddf994) rebuild into braces accounting for their surface, and the guard
/// keeps them.
const MAX_BRACE_DIAMETER_MM: f32 = 3.0;
/// Kickstand detection: a leg stands on a bottom of an untraced piece below the root top (on the
/// plate or a raft) and runs up to where the piece touches a support, straight or leaning off the
/// top of a column rising from that bottom. Its leaning strut is at least `MIN_KICKSTAND_MM` long
/// and leans between `MIN_KICKSTAND_LEAN` and `MAX_KICKSTAND_LEAN` (horizontal run per unit of
/// height, about 20 to 63 degrees), and the piece covers it as a brace's line is covered. Styles:
/// lychee-trees, lychee-lattice, and Eiren's Wednesday in pad12-pillars. Sweep: no effect (each).
const MIN_KICKSTAND_MM: f32 = 2.0;
/// See [`MIN_KICKSTAND_MM`].
const MIN_KICKSTAND_LEAN: f32 = 0.36;
/// See [`MIN_KICKSTAND_MM`].
const MAX_KICKSTAND_LEAN: f32 = 2.0;
/// Where a strut meets its support is looked for every `KNOT_STEP_MM` along the junction. Sweep: no
/// effect.
const KNOT_STEP_MM: f32 = 0.5;
/// A column carries on above a junction a traced support's walk passed through when its own slices
/// go on within this height above where the junction cuts it. Safety limit: junctions are about as
/// tall as the brace in them, well under this; it stops the search where a column ends in a
/// junction and nothing of its own carries on.
const COLUMN_JUNCTION_MM: f32 = 2.0;
/// DragonFruit's own root size, not a heuristic.
pub const ROOT_DISK_HEIGHT_MM: f32 = 0.5;
/// DragonFruit's own root size, not a heuristic.
pub const ROOT_CONE_HEIGHT_MM: f32 = 1.5;
/// Where a plate support's shaft leaves its root (DragonFruit's root height).
pub const ROOT_TOP_MM: f32 = ROOT_DISK_HEIGHT_MM + ROOT_CONE_HEIGHT_MM;
/// Root diameter as a multiple of the shaft diameter, within the min/max. Styles: every style
/// standing on the plate. Sweep: no effect on the fit; the root size only moves how many tips
/// DragonFruit's build matches (2.25: about 100 more over the corpus, 4.0: 65 fewer).
const ROOT_DIAMETER_RATIO: f32 = 3.0;
/// See [`ROOT_DIAMETER_RATIO`].
const MIN_ROOT_DIAMETER_MM: f32 = 2.0;
/// See [`ROOT_DIAMETER_RATIO`].
const MAX_ROOT_DIAMETER_MM: f32 = 6.0;
/// The socket must sit at least this far above the root top for a shaft to fit. Sweep: no effect.
const MIN_SHAFT_LENGTH_MM: f32 = 0.2;

/// No blob or support: a marker, not a heuristic.
const NONE: u32 = u32::MAX;

/// Support vertices this close are one vertex when joining triangles into shells. A numeric
/// tolerance.
const SHELL_WELD_MM: f32 = 0.001;
/// Shells with more triangles than this are never a single cone. A safety limit on cost: such
/// shells (a whole unioned support tree) fail the cone fit anyway (none in the corpus or the guard
/// sets fits), and fitting them is skipped.
const MAX_CONE_TRIANGLES: usize = 2_000;
/// A cone's ends are read from the vertices within this share of its length of each end. Styles:
/// chitubox-trees, lychee-zigzag and hollow-struts (tips that are cones of their own). Sweep:
/// fragile at 0.05 (0.0375 read too few vertices, recall and precision at 0.1 mm down 0.01).
/// Retuned in the audit from 0.05: a tenth fits as well in every style over the 158 round-trip and
/// benchmark models, with 4% fewer joints per support (16% on Lychee zigzag webs).
const CONE_END_BAND: f32 = 0.1;
/// A cone tapers: its narrow end is at most this share of its wide end. Sweep: no effect.
const CONE_TAPER: f32 = 0.85;
/// Vertices lie this close to a cone's wall, plus a share of its wide radius. Sweep: no effect
/// (each).
const CONE_TOLERANCE_MM: f32 = 0.02;
/// See [`CONE_TOLERANCE_MM`].
const CONE_TOLERANCE_RATIO: f32 = 0.05;

/// The shortest brace or strut of `diameter`, and how far its ends are looked for along the
/// shafts it meets: thin braces packed close in a lattice are shorter than thick ones.
fn min_brace_mm(diameter: f32) -> f32 {
    scaled(diameter, MIN_BRACE_RATIO, MIN_BRACE_MIN_MM)
}

/// How far beyond its shaft's radius a brace end of `diameter` may lie for a knot to hang it
/// there (see [`KNOT_REACH_RATIO`]).
fn knot_reach_mm(diameter: f32) -> f32 {
    scaled(diameter, KNOT_REACH_RATIO, KNOT_REACH_MIN_MM)
}

/// `feature * ratio`, and at least `floor`: a tolerance that grows with a measured size of the
/// support (its shaft, tip or brace) past the size it was tuned on.
fn scaled(feature: f32, ratio: f32, floor: f32) -> f32 {
    (feature * ratio).max(floor)
}

/// Coverage guard: the rebuilt supports account for a support's surface within this distance of
/// their own. Styles: the guard, which rejects the unsupported group. Sweep: no effect.
const COVERAGE_MARGIN_MM: f32 = 0.3;
/// Support surface is counted from this far above the raft (or the root tops, if higher); rafts and
/// roots are not rebuilt as pieces. Sweep: no effect.
const COVERAGE_FLOOR_MM: f32 = 0.5;
/// Support triangles are sampled about this far apart, at most [`MAX_COVERAGE_SAMPLES`] each.
/// Sweep: no effect (each).
const COVERAGE_SAMPLE_MM: f32 = 0.25;
/// See [`COVERAGE_SAMPLE_MM`].
const MAX_COVERAGE_SAMPLES: usize = 16;
/// A lookup grid: no effect on any result.
const COVERAGE_CELL_MM: f32 = 2.0;
/// Real supports rebuild into supports accounting for at least this share of their surface (82% or
/// more on the sample pre-supported models); separate parts of a model without supports (rivets,
/// straps, panels) mostly rebuild into little (18% at the median). Guard threshold with a wide
/// margin: nothing flips from 0.6 to 0.88.
const MIN_SUPPORT_COVERAGE: f32 = 0.7;
/// Supports shorter than this (from the tip's contact to where the shaft starts) are not counted
/// towards holding the model up: rivets and pegs rebuild into stubs. Guard threshold with a wide
/// margin: nothing flips from 0.75 to 2.5.
const HOLDING_MIN_LENGTH_MM: f32 = 1.0;
/// Real supports hold the model up: at least this share of the supports (counting those at least
/// [`HOLDING_MIN_LENGTH_MM`] long) have tips that do (56% or more on the sample pre-supported
/// models, where the rest mostly lean on side faces). Separate parts of a model without supports
/// that rebuild like supports stick out of side faces (pegs, spikes), sit inside the model (pins,
/// panels) or rest against a side face beside the part they meet (a casing under its bullet), so at
/// most 6% of theirs do. Guard threshold with a wide margin: nothing flips from 0.05 to 0.55.
const MIN_HOLDING_SHARE: f32 = 0.25;

/// The tip where a support touches the model.
#[derive(Debug, Clone, PartialEq)]
pub struct ContactTip {
    /// Point on the model surface.
    pub contact: Vec3,
    /// Model surface normal at the contact, facing the support.
    pub surface_normal: Vec3,
    /// The support meets the model surface from the side it faces outward. A real support
    /// always does; a part inside the model (a pin through it) meets it from within.
    pub from_outside: bool,
    /// Where the tip meets the shaft.
    pub socket: Vec3,
    /// Width where the tip leaves the model, which is where DragonFruit starts it: the part of
    /// a tip buried in the model prints as part of the model.
    pub contact_diameter_mm: f32,
    pub body_diameter_mm: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SupportBase {
    /// A root on the plate; the shaft leaves the root's top ([`ROOT_TOP_MM`]) at `(x, y)`.
    Plate { x: f32, y: f32, diameter_mm: f32 },
    /// Hangs from `point` on the shaft of an earlier support.
    Shaft { parent: usize, point: Vec3 },
    /// A strut between two parts of the model: the shaft starts at a second, lower contact.
    Model(ContactTip),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SupportPath {
    pub tip: ContactTip,
    pub base: SupportBase,
    /// Bends between the base and the socket, bottom to top.
    pub joints: Vec<Vec3>,
    pub shaft_diameter_mm: f32,
}

impl SupportPath {
    /// Where the shaft starts: the root's top or the knot on the parent shaft.
    pub fn shaft_start(&self) -> Vec3 {
        match &self.base {
            SupportBase::Plate { x, y, .. } => Vec3::new(*x, *y, ROOT_TOP_MM),
            SupportBase::Shaft { point, .. } => *point,
            SupportBase::Model(lower) => lower.socket,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SupportLayout {
    /// A support's parent always comes before it.
    pub supports: Vec<SupportPath>,
    /// Posts with no contact of their own, numbered on from the supports wherever a support is
    /// named by index: `supports.len() + i` is `posts[i]`.
    pub posts: Vec<PostLayout>,
    pub braces: Vec<BraceLayout>,
    pub kickstands: Vec<KickstandLayout>,
}

impl SupportLayout {
    /// Whether the layout accounts for the support geometry it was rebuilt from as supports:
    /// it covers their surface (see [`MIN_SUPPORT_COVERAGE`]) and its supports hold the model
    /// up (see [`MIN_HOLDING_SHARE`]). False when the geometry classed as supports is really
    /// separate parts of a model without supports.
    pub fn accounts_for(&self, supports: &[Triangle], raft_height_mm: f32) -> bool {
        self.coverage(supports, raft_height_mm).is_some_and(|c| c >= MIN_SUPPORT_COVERAGE)
            && self.holding_share().is_some_and(|share| share >= MIN_HOLDING_SHARE)
    }

    /// The shaft diameter of the support, or the post past them, that `support` names.
    pub fn shaft_diameter_mm(&self, support: usize) -> f32 {
        match self.supports.get(support) {
            Some(s) => s.shaft_diameter_mm,
            None => self.posts[support - self.supports.len()].diameter_mm,
        }
    }

    /// Share of the supports at least [`HOLDING_MIN_LENGTH_MM`] long that hold the model up:
    /// the tip meets a downward-facing model surface from outside, below it, and a strut stands
    /// on an upward-facing one (a strut whose lower end meets a side face holds nothing up).
    /// None when no support is that long.
    pub fn holding_share(&self) -> Option<f32> {
        let (mut counted, mut holding) = (0usize, 0usize);
        for s in &self.supports {
            if (s.tip.contact - s.shaft_start()).length() < HOLDING_MIN_LENGTH_MM {
                continue;
            }
            counted += 1;
            let stands = match &s.base {
                SupportBase::Model(lower) => lower.from_outside && lower.surface_normal.z > MIN_FACE_TILT,
                SupportBase::Plate { .. } | SupportBase::Shaft { .. } => true,
            };
            holding += usize::from(stands && s.tip.from_outside && s.tip.surface_normal.z < -MIN_FACE_TILT);
        }
        (counted > 0).then(|| holding as f32 / counted as f32)
    }

    /// Share of the support surface above the raft (by area) within a rebuilt shaft, tip,
    /// brace or kickstand. None when no support surface rises above the raft.
    pub fn coverage(&self, supports: &[Triangle], raft_height_mm: f32) -> Option<f32> {
        let pieces = self.pieces();
        // Each piece is listed in every cell a point within its reach can fall in: the cells
        // around samples along it no more than a cell apart.
        let key = |p: Vec3| {
            let k = |v: f32| (v / COVERAGE_CELL_MM).floor() as i32;
            (k(p.x), k(p.y), k(p.z))
        };
        let mut cells: FxHashMap<(i32, i32, i32), Vec<u32>> = FxHashMap::default();
        for (i, &(a, b, radius)) in pieces.iter().enumerate() {
            let steps = ((b - a).length() / COVERAGE_CELL_MM).ceil().max(1.0) as usize;
            let reach = Vec3::new(1.0, 1.0, 1.0) * (radius + COVERAGE_MARGIN_MM + COVERAGE_CELL_MM / 2.0);
            for k in 0..=steps {
                let p = a + (b - a) * (k as f32 / steps as f32);
                let (lo, hi) = (key(p - reach), key(p + reach));
                for x in lo.0..=hi.0 {
                    for y in lo.1..=hi.1 {
                        for z in lo.2..=hi.2 {
                            let listed = cells.entry((x, y, z)).or_default();
                            if listed.last() != Some(&(i as u32)) {
                                listed.push(i as u32);
                            }
                        }
                    }
                }
            }
        }
        let covered = |p: Vec3| {
            cells.get(&key(p)).is_some_and(|listed| {
                listed.iter().any(|&i| {
                    let (a, b, radius) = pieces[i as usize];
                    distance_to_segment(p, a, b) <= radius + COVERAGE_MARGIN_MM
                })
            })
        };

        // Samples, not whole triangles, are counted from the floor up: a shaft is often one
        // ring of triangles from the plate to its top.
        let floor = raft_height_mm.max(ROOT_TOP_MM) + COVERAGE_FLOOR_MM;
        let (mut total, mut rebuilt) = (0.0f64, 0.0f64);
        for t in supports {
            if t.v0.z.max(t.v1.z).max(t.v2.z) <= floor {
                continue;
            }
            let (u, v) = (t.v1 - t.v0, t.v2 - t.v0);
            let area = f64::from(u.cross(v).length()) / 2.0;
            let n = ((2.0 * area).sqrt() as f32 / COVERAGE_SAMPLE_MM).ceil().clamp(1.0, MAX_COVERAGE_SAMPLES as f32)
                as usize;
            let (mut above, mut hits, mut samples) = (0usize, 0usize, 0usize);
            for i in 0..n {
                for j in 0..n - i {
                    let p = t.v0 + u * ((i as f32 + 1.0 / 3.0) / n as f32) + v * ((j as f32 + 1.0 / 3.0) / n as f32);
                    samples += 1;
                    if p.z > floor {
                        above += 1;
                        hits += usize::from(covered(p));
                    }
                }
            }
            total += area * above as f64 / samples as f64;
            rebuilt += area * hits as f64 / samples as f64;
        }
        (total > 0.0).then(|| (rebuilt / total) as f32)
    }

    /// Every rebuilt piece as a segment with its radius: shafts, tips, posts, braces and
    /// kickstands.
    fn pieces(&self) -> Vec<Shaft> {
        let mut pieces = Vec::new();
        for s in &self.supports {
            let line = shaft_line(s);
            pieces.extend(line.windows(2).map(|w| (w[0], w[1], s.shaft_diameter_mm / 2.0)));
            pieces.push((s.tip.socket, s.tip.contact, s.tip.body_diameter_mm / 2.0));
            if let SupportBase::Model(lower) = &s.base {
                pieces.push((lower.socket, lower.contact, lower.body_diameter_mm / 2.0));
            }
        }
        for post in &self.posts {
            pieces.extend(post.line().windows(2).map(|w| (w[0], w[1], post.diameter_mm / 2.0)));
        }
        pieces.extend(self.braces.iter().map(|b| (b.lower.point, b.upper.point, b.diameter_mm / 2.0)));
        for k in &self.kickstands {
            let mut line = vec![k.foot];
            line.extend(k.joints.iter().copied());
            line.push(k.knot);
            pieces.extend(line.windows(2).map(|w| (w[0], w[1], k.diameter_mm / 2.0)));
        }
        pieces
    }
}

/// A post standing on its own root with no contact at its top, which kickstands and braces
/// hang from: a column of support geometry that legs splaying down to the plate lean against
/// (a kickstand cannot hold up another kickstand), or that stands free, only braced to the
/// supports around it. DragonFruit's trunk without a contact cone.
#[derive(Debug, Clone, PartialEq)]
pub struct PostLayout {
    /// Where the post leaves its root's top ([`ROOT_TOP_MM`]); the root stands on the plate below.
    pub foot: Vec3,
    /// Bends between the foot and the top, bottom to top.
    pub joints: Vec<Vec3>,
    /// The top of the column, or where it runs into the shaft of a support right above it.
    pub top: Vec3,
    pub root_diameter_mm: f32,
    pub diameter_mm: f32,
}

impl PostLayout {
    /// The post's centre line from its foot up to its top.
    pub fn line(&self) -> Vec<Vec3> {
        let mut line = vec![self.foot];
        line.extend(self.joints.iter().copied());
        line.push(self.top);
        line
    }
}

/// A bar between two supports, each end on its support's shaft.
#[derive(Debug, Clone, PartialEq)]
pub struct BraceLayout {
    pub lower: BraceEnd,
    pub upper: BraceEnd,
    pub diameter_mm: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BraceEnd {
    /// Index into [`SupportLayout::supports`] (or on into [`SupportLayout::posts`]): the support
    /// or post the end sits on, or the one holding up the kickstand it sits on.
    pub support: usize,
    /// Index into [`SupportLayout::kickstands`] when the end sits on a kickstand.
    pub kickstand: Option<usize>,
    /// A point on or next to the shaft; the knot goes on the shaft nearest it.
    pub point: Vec3,
}

impl BraceEnd {
    /// Whether two ends sit on the same support or kickstand.
    fn same_host(&self, other: &BraceEnd) -> bool {
        (self.support, self.kickstand) == (other.support, other.kickstand)
    }
}

/// A leg from a root on the plate up to the shaft of the support it holds up.
#[derive(Debug, Clone, PartialEq)]
pub struct KickstandLayout {
    /// Index into [`SupportLayout::supports`] (or on into [`SupportLayout::posts`]) of the
    /// trunk, branch or post the leg holds up.
    pub support: usize,
    /// Where the leg meets that support's shaft; the knot goes on the shaft nearest it.
    pub knot: Vec3,
    /// Bends between the foot and the knot, bottom to top: the top of a column rising
    /// straight from the root, where the leg leans off towards the knot.
    pub joints: Vec<Vec3>,
    /// Where the leg leaves its root's top ([`ROOT_TOP_MM`]); the root stands on the plate below.
    pub foot: Vec3,
    pub root_diameter_mm: f32,
    pub diameter_mm: f32,
}

/// Increment when the reconstruction changes, so anything derived from a layout is rebuilt.
pub const CURRENT_SUPPORT_LAYOUT_VERSION: i32 = 19;

/// Rebuilds the support layout from Z-up support geometry and the model it holds up.
pub fn reconstruct(model: &[Triangle], supports: &[Triangle]) -> SupportLayout {
    let shells = Shells::new(supports);
    let Some(graph) = Graph::slice(supports, &shells.of) else {
        return SupportLayout::default();
    };
    let surface = SurfaceIndex::new(model);

    let mut tops: Vec<(u32, SurfacePoint)> = (0..graph.blobs.len() as u32)
        .filter(|&i| graph.up[i as usize].is_empty())
        .filter_map(|i| surface.contact(graph.centre(i)).map(|hit| (i, hit)))
        .collect();
    tops.sort_by(|a, b| graph.blobs[b.0 as usize].z.total_cmp(&graph.blobs[a.0 as usize].z));

    let mut claimed = vec![NONE; graph.blobs.len()];
    // Every blob any walk passed, and the traced support that passed it first.
    let mut seen = vec![false; graph.blobs.len()];
    let mut owner = vec![NONE; graph.blobs.len()];
    let mut layout = SupportLayout::default();
    let (mut stranded, mut duplicate, mut too_low) = (0usize, 0usize, 0usize);
    for (top, top_contact) in tops {
        // The walk keeps off the blobs of support `avoid`, when given.
        let traced = |pass_beside: bool, avoid: Option<usize>| {
            let open = |i: u32| avoid.is_none_or(|a| claimed[i as usize] != a as u32 && owner[i as usize] != a as u32);
            let walk = graph.walk(top, &claimed, pass_beside, open);
            let points: Vec<Vec3> = walk.nodes.iter().map(|&i| graph.centre(i)).collect();
            let (contact, entry) = surface.entry(&points).unwrap_or((top_contact, 0));
            let lower = match walk.end {
                WalkEnd::Stranded => lower_end(&graph, &surface, &shells, &walk),
                _ => None,
            };
            let outcome = if matches!(walk.end, WalkEnd::Stranded) && lower.is_none() {
                Traced::Stranded
            } else if second_top(&graph, &layout.supports, &walk, contact.point, &claimed, &owner) {
                Traced::Duplicate
            } else {
                let cone = shells.cone(graph.blobs[top as usize].shell);
                build_support(&graph, &walk, &contact, entry, lower, cone).map_or(Traced::TooLow, Traced::Support)
            };
            (walk, contact, outcome)
        };
        // Passing up the ways beside the shaft can lead the walk into a dead end that loses the
        // support (a sliver of its tip); then it may take them.
        let (mut walk, mut contact, mut outcome) = traced(true, None);
        if matches!(outcome, Traced::Stranded | Traced::TooLow) {
            (walk, contact, outcome) = traced(false, None);
        }
        // A strut packed beside another one can run into it where their tips, feet or a rung
        // merge them: when the walk keeps off that strut, it reaches the model on a foot of its
        // own. Kept off a support, a walk can also wander through rungs and braces onto another
        // strut's way down, which bends more than the branch it would replace.
        if let (WalkEnd::Joined(parent, _), Traced::Support(branch)) = (&walk.end, &outcome)
            && let SupportBase::Model(parent_foot) = &layout.supports[*parent].base
        {
            let own_foot = traced(true, Some(*parent));
            if let Traced::Support(strut @ SupportPath { base: SupportBase::Model(foot), .. }) = &own_foot.2
                && (foot.contact - parent_foot.contact).length() >= DUPLICATE_CONTACT_MM
                && strut.joints.len() <= branch.joints.len()
            {
                (walk, contact, outcome) = own_foot;
            }
        }
        log::trace!(
            "Tip candidate top {} contact {} ({} shaft blobs, ends {})",
            graph.centre(top),
            contact.point,
            walk.nodes.len(),
            match walk.end {
                WalkEnd::Plate => "on the plate".to_string(),
                WalkEnd::Joined(parent, _) => format!("on support {parent}"),
                WalkEnd::Stranded => "stranded".to_string(),
            }
        );
        for &node in &walk.visited {
            seen[node as usize] = true;
        }
        let support = match outcome {
            Traced::Support(support) => Some(support),
            Traced::Stranded => {
                stranded += 1;
                None
            }
            Traced::Duplicate => {
                duplicate += 1;
                None
            }
            Traced::TooLow => {
                too_low += 1;
                None
            }
        };
        if let Some(support) = support {
            let id = layout.supports.len() as u32;
            for &node in &walk.nodes {
                claimed[node as usize] = id;
            }
            for &node in &walk.visited {
                if owner[node as usize] == NONE {
                    owner[node as usize] = id;
                }
            }
            layout.supports.push(support);
        }
    }
    let mut pieces = graph.untraced(&seen, &owner, layout.supports.len(), &[]);
    // Posts that legs lean against stand on their own, so the legs can hang from them: their
    // slices are taken out of the untraced geometry, as a traced support's are, but for those
    // around the root, where other legs splaying from the post's foot stand or pass it.
    let posts = graph.posts(&pieces, &seen, &owner, &host_lines(&layout.supports, &[]));
    for (p, (post, piece)) in posts.iter().enumerate() {
        for i in graph.along(&pieces[*piece], &post.line()) {
            if !seen[i as usize] && graph.blobs[i as usize].z >= ROOT_TOP_MM + THROUGH_ROOT_MM {
                seen[i as usize] = true;
                owner[i as usize] = (layout.supports.len() + p) as u32;
            }
        }
    }
    layout.posts = posts.into_iter().map(|(post, _)| post).collect();
    let hosts = host_lines(&layout.supports, &layout.posts);
    if !layout.posts.is_empty() {
        pieces = graph.untraced(&seen, &owner, hosts.len(), &[]);
    }
    let (mut shafts, mut shaft_hosts) = traced_shafts(&hosts);
    graph.braces(&pieces, &shafts, &shaft_hosts, &mut layout.braces, false);
    let kickstands = graph.kickstands(&pieces, &seen, &owner, &hosts, &shafts);
    // Braces with an end on a kickstand: each kickstand's own slices are taken out of the
    // untraced geometry, as a traced support's are.
    for (k, (kickstand, piece)) in kickstands.iter().enumerate() {
        let mut line = vec![kickstand.foot];
        line.extend(kickstand.joints.iter().copied());
        line.push(kickstand.knot);
        for i in graph.along(&pieces[*piece], &line) {
            if !seen[i as usize] {
                seen[i as usize] = true;
                owner[i as usize] = (hosts.len() + k) as u32;
            }
        }
        shafts.extend(line.windows(2).map(|w| (w[0], w[1], kickstand.diameter_mm / 2.0)));
        shaft_hosts.resize(shafts.len(), ((kickstand.support, Some(k)), true));
    }
    layout.kickstands = kickstands.into_iter().map(|(k, _)| k).collect();
    if !layout.kickstands.is_empty() {
        let pieces = graph.untraced(&seen, &owner, hosts.len(), &layout.kickstands);
        graph.braces(&pieces, &shafts, &shaft_hosts, &mut layout.braces, true);
    }
    log::debug!(
        "Support layout: {} supports, {} posts, {} braces and {} kickstands from {} support triangles ({} stranded, {} duplicate tips, {} too low)",
        layout.supports.len(),
        layout.posts.len(),
        layout.braces.len(),
        layout.kickstands.len(),
        supports.len(),
        stranded,
        duplicate,
        too_low,
    );
    layout
}

/// Whether a walk's contact is a second top of an earlier support's tip (see
/// [`DUPLICATE_CONTACT_MM`]): `claimed` and `owner` hold the support that took each blob into its
/// shaft and the one whose walk passed it first.
fn second_top(
    graph: &Graph,
    supports: &[SupportPath],
    walk: &Walk,
    contact: Vec3,
    claimed: &[u32],
    owner: &[u32],
) -> bool {
    let near: Vec<u32> = (0..supports.len() as u32)
        .filter(|&i| (supports[i as usize].tip.contact - contact).length() < DUPLICATE_CONTACT_MM)
        .collect();
    // Where the walk ends, or first runs into a nearby support's blobs.
    let meets = walk
        .visited
        .iter()
        .find(|&&i| near.contains(&claimed[i as usize]) || near.contains(&owner[i as usize]))
        .unwrap_or(walk.visited.last().expect("a walk has a start"));
    !near.is_empty() && (graph.centre(*meets) - contact).length() < OWN_SUPPORT_MIN_MM
}

/// What a walk from a contact tip came to.
enum Traced {
    Support(SupportPath),
    /// It stopped in mid-air.
    Stranded,
    /// Its contact is a second top of an earlier support's tip.
    Duplicate,
    /// Its socket is too low for a shaft to leave a root.
    TooLow,
}

/// The far end of a walk that stopped short of the plate, when it stops in the model.
struct LowerEnd {
    contact: SurfacePoint,
    /// The walk's blobs from the bottom up.
    nodes: Vec<u32>,
    /// The first of `nodes` outside the model.
    entry: usize,
    /// The shell the lower tip's buried end belongs to, when that is a cone.
    cone: Option<Cone>,
}

/// Where a walk that stopped short of the plate goes into the model, for a strut between two
/// parts of it. None when it stopped in mid-air.
fn lower_end(graph: &Graph, surface: &SurfaceIndex, shells: &Shells, walk: &Walk) -> Option<LowerEnd> {
    let nodes: Vec<u32> = walk.visited.iter().rev().copied().collect();
    surface.contact(graph.centre(nodes[0]))?;
    let points: Vec<Vec3> = nodes.iter().map(|&i| graph.centre(i)).collect();
    let (contact, entry) = surface.entry(&points)?;
    let cone = shells.cone(graph.blobs[nodes[0] as usize].shell);
    Some(LowerEnd { contact, nodes, entry, cone })
}

/// A tip cone fitted to blobs running away from its contact.
#[derive(Clone, Copy)]
struct TipFit {
    /// The socket, counted from the first blob outside the model.
    socket_index: usize,
    socket: Vec3,
    socket_width: f32,
    contact_diameter: f32,
}

/// Fits a tip to `nodes`, which run from the tip's buried end outward; `entry` is the first
/// one outside the model, and `walked` says which of them the walk took (it skips the slices
/// of a tip leaning away from its contact). The tip may take up `share` of the length from the
/// contact on, which runs on to `end` when the support goes past the last node (a branch's
/// knot).
fn fit_tip(
    graph: &Graph,
    nodes: &[u32],
    walked: &[bool],
    entry: usize,
    contact: Vec3,
    end: Option<Vec3>,
    share: f32,
) -> TipFit {
    let points: Vec<Vec3> = nodes[entry..].iter().map(|&i| graph.centre(i)).collect();
    let widths: Vec<f32> = nodes[entry..].iter().map(|&i| graph.blobs[i as usize].half_min * 2.0).collect();
    let walked = &walked[entry..];
    let last = *points.last().expect("a tip has a blob outside the model");
    let total = (points[0] - contact).length()
        + points.windows(2).map(|w| (w[1] - w[0]).length()).sum::<f32>()
        + end.map_or(0.0, |end| (end - last).length());
    // The tip is a straight cone, so it is measured by straight distance from the contact:
    // the blob centres of a steeply inclined tip jump about from slice to slice.
    let reach: Vec<f32> = points.iter().map(|p| (*p - contact).length()).collect();

    let window = TIP_WINDOW_MM.min(total * share);
    let widest = (0..points.len()).take_while(|&i| reach[i] <= window).map(|i| widths[i]).fold(0.0, f32::max);
    let plateau_span = scaled(widest, PLATEAU_SPAN_RATIO, PLATEAU_SPAN_MIN_MM);
    let plateau_growth = scaled(widest, PLATEAU_GROWTH_RATIO, PLATEAU_GROWTH_MIN_MM);
    // A slice of a leaning tip must also keep its width to count as a plateau: past a joint
    // ball the tip runs into, the slices narrow again, and the shaft the walk picks up below the
    // tip is never narrower than it.
    let plateau = |i: usize| {
        (i..points.len()).find(|&j| reach[j] >= reach[i] + plateau_span).is_some_and(|j| {
            let least = match (walked[i], walked[j]) {
                (true, _) => f32::NEG_INFINITY,
                (false, false) => -plateau_growth,
                (false, true) => 0.0,
            };
            (least..plateau_growth).contains(&(widths[j] - widths[i]))
        })
    };
    // A leaning tip's own slices widen to nearly the widest before it ends, so among them only
    // where they stop widening marks the socket.
    let body = |i: usize| walked[i] && widths[i] >= widest * TIP_BODY_FRACTION;
    let socket_index = (0..points.len())
        .take_while(|&i| reach[i] <= window)
        .find(|&i| reach[i] >= MIN_TIP_LENGTH_MM && (body(i) || plateau(i)))
        .or_else(|| (0..points.len()).rev().find(|&i| reach[i] <= window))
        .unwrap_or(0);

    let tip_widths: Vec<f32> =
        nodes[..=entry + socket_index].iter().map(|&i| graph.blobs[i as usize].half_min * 2.0).collect();
    let narrow_end = tip_widths.windows(2).find(|w| w[0] >= w[1] * CAP_WIDTH_FRACTION).map_or(tip_widths[0], |w| w[0]);
    // A tip buried in the model is as wide as its first slice outside it where it leaves it.
    let at_surface = if entry > 0 { tip_widths[entry].min(widths[socket_index]) } else { narrow_end };
    let contact_diameter = narrow_end.max(at_surface).clamp(MIN_CONTACT_DIAMETER_MM, MAX_CONTACT_DIAMETER_MM);
    TipFit { socket_index, socket: points[socket_index], socket_width: widths[socket_index], contact_diameter }
}

/// The tip fit of a tip that is a cone shell of its own: the socket is the cone's wide end, the
/// contact is as wide as the cone where it leaves the model (its narrow end, when that is not
/// buried) and the socket as its wide end. `nodes` run from the first blob outside the model
/// outward; the socket's index is the last of them no farther from the contact than the socket.
fn cone_fit(graph: &Graph, nodes: &[u32], contact: Vec3, cone: &Cone) -> TipFit {
    let length = (cone.wide - contact).length();
    let socket_index = nodes.iter().rposition(|&i| (graph.centre(i) - contact).length() <= length).unwrap_or(0);
    let axis = cone.wide - cone.narrow;
    let along = ((contact - cone.narrow).dot(axis) / axis.dot(axis).max(f32::EPSILON)).clamp(0.0, 1.0);
    let at_contact = cone.narrow_radius + (cone.wide_radius - cone.narrow_radius) * along;
    TipFit {
        socket_index,
        socket: cone.wide,
        socket_width: cone.wide_radius * 2.0,
        contact_diameter: (at_contact * 2.0).clamp(MIN_CONTACT_DIAMETER_MM, MAX_CONTACT_DIAMETER_MM),
    }
}

/// `fit` measured from the tip's own cone shell instead, when it has one whose narrow end is at
/// the contact and whose wide end stands out of the model (the rounded end of a rod buried in
/// the model tapers too). `nodes` run from the first blob outside the model outward. The shaft
/// line still starts below the slices fitted as the tip: those around the joint the cone ends
/// in are off the shaft's line.
fn with_cone(graph: &Graph, nodes: &[u32], contact: Vec3, cone: Option<Cone>, fit: TipFit) -> TipFit {
    let tip = |c: &Cone| {
        (c.narrow - contact).length() <= CONTACT_MAX_MM
            && (c.wide - contact).dot((c.wide - c.narrow).normalized()) >= MIN_TIP_LENGTH_MM
    };
    match cone.filter(tip) {
        Some(cone) => {
            let cone_fit = cone_fit(graph, nodes, contact, &cone);
            TipFit { socket_index: fit.socket_index.max(cone_fit.socket_index), ..cone_fit }
        }
        None => fit,
    }
}

/// The contact tip for a fit, with the surface normal turned to face the support.
fn contact_tip(contact: &SurfacePoint, fit: &TipFit, shaft_diameter: f32) -> ContactTip {
    let from_outside = contact.normal.dot(fit.socket - contact.point) >= 0.0;
    let surface_normal = if from_outside { contact.normal } else { -contact.normal };
    // DragonFruit never makes a tip wider than its shaft.
    let contact_diameter = fit.contact_diameter.min(shaft_diameter);
    ContactTip {
        contact: contact.point,
        surface_normal,
        from_outside,
        socket: fit.socket,
        contact_diameter_mm: contact_diameter,
        body_diameter_mm: fit.socket_width.min(shaft_diameter).max(contact_diameter),
    }
}

/// The shaft diameter from its cross-sections: a shaft keeps one width, and joint balls, root
/// flares and merged braces only ever widen a section.
fn shaft_diameter(mut widths: Vec<f32>, fallback: f32) -> f32 {
    widths.sort_by(f32::total_cmp);
    widths.get(widths.len() / SHAFT_WIDTH_QUANTILE).copied().unwrap_or(fallback).max(MIN_ROD_DIAMETER_MM)
}

/// Builds a support from its walk. `entry` is the first walked blob outside the model; the
/// blobs above it are the part of the tip buried in the model. `cone` is the shell the tip's
/// buried end belongs to, when that is a cone.
fn build_support(
    graph: &Graph,
    walk: &Walk,
    contact: &SurfacePoint,
    entry: usize,
    lower: Option<LowerEnd>,
    cone: Option<Cone>,
) -> Option<SupportPath> {
    if let Some(lower) = lower {
        return Some(build_strut(graph, walk, contact, entry, &lower, cone));
    }
    let entry = walk.tip_index(entry);
    let knot = match walk.end {
        WalkEnd::Joined(_, node) => Some(graph.centre(node)),
        _ => None,
    };
    let (nodes, walked) = walk.tip();
    let fit = fit_tip(graph, &nodes, &walked, entry, contact.point, knot, TIP_SHARE);
    // A cone reaching down into the root is the whole support rather than its tip.
    let leaves_shaft = |c: &Cone| !matches!(walk.end, WalkEnd::Plate) || c.wide.z >= ROOT_TOP_MM + MIN_SHAFT_LENGTH_MM;
    let fit = with_cone(graph, &nodes[entry..], contact.point, cone.filter(leaves_shaft), fit);
    let below = walk.nodes_below(entry + fit.socket_index);
    let shaft = shaft_slices(graph, below);
    let widths: Vec<f32> = shaft
        .iter()
        .map(|&i| &graph.blobs[i as usize])
        .filter(|b| b.z >= ROOT_TOP_MM)
        .map(|b| b.half_min * 2.0)
        .collect();
    let shaft_diameter = shaft_diameter(widths, fit.socket_width);
    let tip = contact_tip(contact, &fit, shaft_diameter);
    let socket = fit.socket;

    // Shaft centre line from the socket down, ending at the base.
    let mut line: Vec<Vec3> = vec![socket];
    line.extend(shaft.iter().map(|&i| graph.centre(i)));
    let base = match walk.end {
        WalkEnd::Joined(parent, node) => {
            let point = graph.centre(node);
            line.push(point);
            SupportBase::Shaft { parent, point }
        }
        WalkEnd::Plate => {
            if socket.z < ROOT_TOP_MM + MIN_SHAFT_LENGTH_MM {
                return None;
            }
            // Cut the line where it passes the root top, so the shaft leaves the root there.
            let above = line.iter().take_while(|p| p.z > ROOT_TOP_MM).count();
            let foot = match line.get(above) {
                Some(&below) if above > 0 => {
                    let upper = line[above - 1];
                    let t = (upper.z - ROOT_TOP_MM) / (upper.z - below.z);
                    upper + (below - upper) * t
                }
                _ => line[above.saturating_sub(1)],
            };
            line.truncate(above.max(1));
            let foot = Vec3::new(foot.x, foot.y, ROOT_TOP_MM);
            line.push(foot);
            let diameter_mm = (shaft_diameter * ROOT_DIAMETER_RATIO).clamp(MIN_ROOT_DIAMETER_MM, MAX_ROOT_DIAMETER_MM);
            SupportBase::Plate { x: foot.x, y: foot.y, diameter_mm }
        }
        WalkEnd::Stranded => return None,
    };

    Some(SupportPath { tip, base, joints: bends(&line, shaft_diameter), shaft_diameter_mm: shaft_diameter })
}

/// A strut between two parts of the model, from its upper contact down to its lower one. Either
/// tip that is a cone shell of its own (`cone` for the upper one) is measured from the cone.
fn build_strut(
    graph: &Graph,
    walk: &Walk,
    contact: &SurfacePoint,
    entry: usize,
    lower: &LowerEnd,
    cone: Option<Cone>,
) -> SupportPath {
    let (nodes, walked) = walk.tip();
    let entry = walk.tip_index(entry);
    let upper_fit = fit_tip(graph, &nodes, &walked, entry, contact.point, None, STRUT_TIP_SHARE);
    let mut upper_fit = with_cone(graph, &nodes[entry..], contact.point, cone, upper_fit);
    let lower_walked = vec![true; lower.nodes.len()];
    let lower_contact = lower.contact.point;
    let lower_fit = fit_tip(graph, &lower.nodes, &lower_walked, lower.entry, lower_contact, None, STRUT_TIP_SHARE);
    let mut lower_fit = with_cone(graph, &lower.nodes[lower.entry..], lower_contact, lower.cone, lower_fit);
    if upper_fit.socket.z <= lower_fit.socket.z {
        // The two tips overlap on a short strut: put each socket a third of the way along.
        let span = lower.contact.point - contact.point;
        upper_fit.socket = contact.point + span * (1.0 / 3.0);
        lower_fit.socket = contact.point + span * (2.0 / 3.0);
    }
    let (upper_socket, lower_socket) = (upper_fit.socket, lower_fit.socket);
    let shaft: Vec<u32> = shaft_slices(graph, &walk.nodes)
        .into_iter()
        .filter(|&i| (lower_socket.z..upper_socket.z).contains(&graph.blobs[i as usize].z))
        .collect();
    let between: Vec<f32> = shaft.iter().map(|&i| graph.blobs[i as usize].half_min * 2.0).collect();
    let shaft_diameter = shaft_diameter(between, upper_fit.socket_width.min(lower_fit.socket_width));

    // Shaft centre line from the upper socket down to the lower one.
    let mut line = vec![upper_socket];
    line.extend(shaft.iter().map(|&i| graph.centre(i)));
    line.push(lower_socket);
    SupportPath {
        tip: contact_tip(contact, &upper_fit, shaft_diameter),
        base: SupportBase::Model(contact_tip(&lower.contact, &lower_fit, shaft_diameter)),
        joints: bends(&line, shaft_diameter),
        shaft_diameter_mm: shaft_diameter,
    }
}

/// The shaft blobs `nodes` (top down) that are the shaft on its own, for its line and width:
/// those notably wider than the blobs around them are a joint ball, or the shaft merged with a
/// brace or another shaft, whose middle can lie off the shaft's line.
fn shaft_slices(graph: &Graph, nodes: &[u32]) -> Vec<u32> {
    let blob = |k: usize| &graph.blobs[nodes[k] as usize];
    let (mut lo, mut hi) = (0, 0);
    let mut around = Vec::new();
    (0..nodes.len())
        .filter(|&k| {
            let z = blob(k).z;
            while blob(lo).z > z + LOCAL_WIDTH_MM {
                lo += 1;
            }
            while hi < nodes.len() && blob(hi).z >= z - LOCAL_WIDTH_MM {
                hi += 1;
            }
            around.clear();
            around.extend((lo..hi).map(|j| blob(j).half_max));
            around.sort_by(f32::total_cmp);
            blob(k).half_max <= around[around.len() / 2] * SHAFT_ALONE_RATIO + SHAFT_ALONE_SLACK_MM
        })
        .map(|k| nodes[k])
        .collect()
}

/// The bends of a shaft line of `diameter` running from the top down, bottom to top.
fn bends(line: &[Vec3], diameter: f32) -> Vec<Vec3> {
    let kept = simplify(line, scaled(diameter, SIMPLIFY_RATIO, SIMPLIFY_MIN_MM));
    kept[1..kept.len() - 1].iter().rev().map(|&i| line[i]).collect()
}

/// Douglas-Peucker: indices of the points kept, always including both ends.
fn simplify(points: &[Vec3], tolerance: f32) -> Vec<usize> {
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (mut worst, mut worst_distance) = (0, tolerance);
        for i in a + 1..b {
            let distance = distance_to_segment(points[i], points[a], points[b]);
            if distance > worst_distance {
                (worst, worst_distance) = (i, distance);
            }
        }
        if worst != 0 {
            keep[worst] = true;
            stack.push((a, worst));
            stack.push((worst, b));
        }
    }
    (0..points.len()).filter(|&i| keep[i]).collect()
}

fn distance_to_segment(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + ab * t)).length()
}

/// The point of segment `ab` closest to the line through `origin` along the unit `direction`.
fn closest_to_line(a: Vec3, b: Vec3, origin: Vec3, direction: Vec3) -> Vec3 {
    let off_line = |v: Vec3| v - direction * v.dot(direction);
    let (w, u) = (off_line(a - origin), off_line(b - a));
    let t = if u.length_sq() > 1e-12 { (-w.dot(u) / u.length_sq()).clamp(0.0, 1.0) } else { 0.5 };
    a + (b - a) * t
}

/// A traced shaft segment: its ends and radius.
type Shaft = (Vec3, Vec3, f32);

/// A support's shaft from where it starts up to the socket.
fn shaft_line(support: &SupportPath) -> Vec<Vec3> {
    let mut line = vec![support.shaft_start()];
    line.extend(support.joints.iter().copied());
    line.push(support.tip.socket);
    line
}

/// A support (index into [`SupportLayout::supports`]), or a kickstand holding it up (index into
/// [`SupportLayout::kickstands`]), as a [`BraceEnd`] names it.
type Host = (usize, Option<usize>);

/// A traced support's or post's shaft, as the kickstands and braces hanging from it see it.
struct HostLine {
    /// From where the shaft starts up to the socket, or the post's top.
    line: Vec<Vec3>,
    /// The tip's contact, past the socket.
    contact: Option<Vec3>,
    radius: f32,
    /// A strut between two parts of the model, which holds up no kickstand.
    strut: bool,
}

/// The shafts of the traced supports, then of the posts, in the order a support index names
/// them.
fn host_lines(supports: &[SupportPath], posts: &[PostLayout]) -> Vec<HostLine> {
    let traced = supports.iter().map(|s| HostLine {
        line: shaft_line(s),
        contact: Some(s.tip.contact),
        radius: s.shaft_diameter_mm / 2.0,
        strut: matches!(s.base, SupportBase::Model(_)),
    });
    let posts =
        posts.iter().map(|p| HostLine { line: p.line(), contact: None, radius: p.diameter_mm / 2.0, strut: false });
    traced.chain(posts).collect()
}

/// Every traced shaft segment, tips included, the support each belongs to, and whether a knot can
/// hang on it (a tip is no shaft for a knot).
fn traced_shafts(hosts: &[HostLine]) -> (Vec<Shaft>, Vec<(Host, bool)>) {
    hosts
        .iter()
        .enumerate()
        .flat_map(|(i, host)| {
            let shaft = host.line.windows(2).map(move |w| ((w[0], w[1], host.radius), ((i, None), true)));
            let socket = *host.line.last().expect("a shaft has two ends");
            let tip = host.contact.map(|contact| ((socket, contact, host.radius), ((i, None), false)));
            shaft.chain(tip)
        })
        .unzip()
}

/// Whether the middle third of a bar from `p` to `q` passes through the centre line of a shaft
/// belonging to neither of `hosts` (see [`THROUGH_SKIP`]).
fn through_shaft(shafts: &[Shaft], shaft_hosts: &[(Host, bool)], hosts: [Host; 2], p: Vec3, q: Vec3) -> bool {
    let (a, b) = (p + (q - p) * THROUGH_SKIP, q - (q - p) * THROUGH_SKIP);
    let steps = (((b - a).length() / BRACE_SAMPLE_MM).ceil() as usize).max(1);
    let apart = |u: Vec3, v: Vec3, reach: f32| {
        [(u.x, v.x, a.x, b.x), (u.y, v.y, a.y, b.y), (u.z, v.z, a.z, b.z)]
            .iter()
            .any(|&(u, v, a, b)| u.max(v) + reach < a.min(b) || u.min(v) - reach > a.max(b))
    };
    shafts.iter().zip(shaft_hosts).any(|(&(s, t, radius), (host, _))| {
        !hosts.contains(host)
            && !apart(s, t, radius)
            && (0..=steps).any(|k| distance_to_segment(a + (b - a) * (k as f32 / steps as f32), s, t) < radius)
    })
}

/// Whether a bar from `p` to `q` lies along a traced shaft all through its middle: then it is
/// a sliver of that shaft rather than a brace or kickstand.
fn along_shaft(shafts: &[Shaft], p: Vec3, q: Vec3, diameter_mm: f32) -> bool {
    let span = q - p;
    let direction = span.normalized();
    (1..4).all(|k| {
        let at = p + span * (k as f32 / 4.0);
        shafts.iter().any(|&(a, b, r)| {
            distance_to_segment(at, a, b) <= r.max(diameter_mm / 2.0)
                && (b - a).normalized().dot(direction).abs() > BRACE_PARALLEL_COS
        })
    })
}

/// An axis-aligned box on a slice plane.
#[derive(Debug, Clone, Copy)]
struct Rect {
    min: [f32; 2],
    max: [f32; 2],
}

impl Rect {
    fn point(p: [f32; 2]) -> Self {
        Self { min: p, max: p }
    }

    fn grow(&mut self, p: [f32; 2]) {
        self.min = [self.min[0].min(p[0]), self.min[1].min(p[1])];
        self.max = [self.max[0].max(p[0]), self.max[1].max(p[1])];
    }

    fn union(&mut self, other: &Rect) {
        self.grow(other.min);
        self.grow(other.max);
    }

    fn centre(&self) -> [f32; 2] {
        [(self.min[0] + self.max[0]) / 2.0, (self.min[1] + self.max[1]) / 2.0]
    }

    fn area(&self) -> f32 {
        (self.max[0] - self.min[0]) * (self.max[1] - self.min[1])
    }

    fn holds(&self, p: [f32; 2], margin: f32) -> bool {
        p[0] >= self.min[0] - margin
            && p[0] <= self.max[0] + margin
            && p[1] >= self.min[1] - margin
            && p[1] <= self.max[1] + margin
    }

    fn encloses(&self, other: &Rect) -> bool {
        self.min[0] <= other.min[0]
            && self.min[1] <= other.min[1]
            && self.max[0] >= other.max[0]
            && self.max[1] >= other.max[1]
    }

    fn overlaps(&self, other: &Rect, margin: f32) -> bool {
        self.min[0] <= other.max[0] + margin
            && other.min[0] <= self.max[0] + margin
            && self.min[1] <= other.max[1] + margin
            && other.min[1] <= self.max[1] + margin
    }

    /// Grid cells (of `size`) the box covers.
    fn cells(&self, size: f32) -> impl Iterator<Item = (i32, i32)> {
        let (x0, y0) = cell2(self.min, size);
        let (x1, y1) = cell2(self.max, size);
        (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| (x, y)))
    }
}

fn cell2(p: [f32; 2], size: f32) -> (i32, i32) {
    ((p[0] / size).floor() as i32, (p[1] / size).floor() as i32)
}

#[derive(Debug, Clone, Copy)]
struct Blob {
    x: f32,
    y: f32,
    z: f32,
    half_min: f32,
    half_max: f32,
    layer: u32,
    rect: Rect,
    /// The support shell the blob's (largest) outline belongs to.
    shell: u32,
}

struct Graph {
    blobs: Vec<Blob>,
    up: Vec<Vec<u32>>,
    down: Vec<Vec<u32>>,
    /// Height of the lowest slice.
    floor: f32,
    /// Height between slices.
    step: f32,
    /// Each blob's [`Graph::descent_slope`], once measured (NaN until then): every walk passing a
    /// fork weighs the same ways down.
    slopes: Vec<Cell<f32>>,
}

enum WalkEnd {
    /// Reached the plate.
    Plate,
    /// Reached a blob claimed by `supports[.0]`, at blob `.1`.
    Joined(usize, u32),
    /// Ran out of blobs above the plate: a strut ending on the model, or a dead end.
    Stranded,
}

struct Walk {
    /// Shaft blobs from the top down, leaving out widened ones.
    nodes: Vec<u32>,
    /// The slices of a tip leaning away from its contact that the walk skipped (see
    /// [`Graph::walk`]); they go between `nodes[..leaning_at]` and the rest.
    leaning: Vec<u32>,
    leaning_at: usize,
    /// Every blob passed, from the top down.
    visited: Vec<u32>,
    end: WalkEnd,
}

impl Walk {
    /// The blobs a tip is fitted to: the shaft blobs with a leaning tip's slices put back, and
    /// whether the walk took each.
    fn tip(&self) -> (Vec<u32>, Vec<bool>) {
        let mut tip = self.nodes[..self.leaning_at].to_vec();
        tip.extend(&self.leaning);
        tip.extend(&self.nodes[self.leaning_at..]);
        let mut walked = vec![true; tip.len()];
        walked[self.leaning_at..self.leaning_at + self.leaning.len()].fill(false);
        (tip, walked)
    }

    /// Where `nodes[i]` is in [`Walk::tip`].
    fn tip_index(&self, i: usize) -> usize {
        if i < self.leaning_at { i } else { i + self.leaning.len() }
    }

    /// The shaft blobs below the `i`th blob of [`Walk::tip`].
    fn nodes_below(&self, i: usize) -> &[u32] {
        let from =
            if i < self.leaning_at { i + 1 } else { (i + 1).saturating_sub(self.leaning.len()).max(self.leaning_at) };
        &self.nodes[from..]
    }
}

/// A connected piece of the support geometry no walk passed.
struct Piece {
    blobs: Vec<u32>,
    /// Where the piece touches traced supports, grouped per support by height: the mean of
    /// each group, and the group's touches from the bottom up.
    ends: Vec<BraceEnd>,
    touches: Vec<Vec<Vec3>>,
    /// The piece's blobs by layer and grid cell ([`BLOB_CELL_MM`]), each in every cell its box
    /// (grown by [`BRACE_MARGIN_MM`]) covers.
    cells: FxHashMap<(u32, i32, i32), Vec<u32>>,
}

/// A column of untraced support geometry rising straight up from the plate: a post.
struct Column {
    /// The top of the column on its own, on its centre line.
    top: Vec3,
    /// Where struts and braces merge into or meet the column below its top, on its centre line,
    /// from the top down.
    junctions: Vec<Vec3>,
    /// The traced support the column runs into above its top, and its centre line there.
    into: Option<(usize, Vec3)>,
    radius: f32,
    /// The widths of the column's own slices, from the foot up.
    widths: Vec<f32>,
}

impl Graph {
    fn slice(triangles: &[Triangle], shell_of: &[u32]) -> Option<Graph> {
        let (lo, hi) = triangles
            .iter()
            .flat_map(|t| [t.v0.z, t.v1.z, t.v2.z])
            .fold((f32::MAX, f32::MIN), |(lo, hi), z| (lo.min(z), hi.max(z)));
        if hi - lo <= LAYER_MM || !(hi - lo).is_finite() {
            return None;
        }
        let step = LAYER_MM.max((hi - lo) / MAX_LAYERS as f32);
        let count = ((hi - lo) / step).floor() as usize;
        let plane = |k: usize| lo + (k as f32 + 0.5) * step;

        let layers: Vec<Vec<Section>> = (0..count)
            .step_by(LAYERS_PER_BAND)
            .collect::<Vec<_>>()
            .into_par_iter()
            .map(|start| {
                let end = (start + LAYERS_PER_BAND).min(count);
                let (band_lo, band_hi) = (plane(start), plane(end - 1));
                let band: Vec<u32> = (0..triangles.len() as u32)
                    .filter(|&i| {
                        let t = &triangles[i as usize];
                        t.v0.z.min(t.v1.z).min(t.v2.z) <= band_hi && t.v0.z.max(t.v1.z).max(t.v2.z) >= band_lo
                    })
                    .collect();
                (start..end).map(|k| layer_blobs(triangles, &band, shell_of, plane(k))).collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
            .into_iter()
            .flatten()
            .collect();

        let mut offsets = Vec::with_capacity(layers.len() + 1);
        let mut blobs = Vec::new();
        for (k, sections) in layers.iter().enumerate() {
            offsets.push(blobs.len() as u32);
            blobs.extend(sections.iter().map(|section| {
                let r = &section.rect;
                let [x, y] = r.centre();
                let (w, h) = (r.max[0] - r.min[0], r.max[1] - r.min[1]);
                Blob {
                    x,
                    y,
                    z: plane(k),
                    half_min: section.width / 2.0,
                    half_max: w.max(h) / 2.0,
                    layer: k as u32,
                    rect: *r,
                    shell: section.shell,
                }
            }));
        }
        offsets.push(blobs.len() as u32);

        let links: Vec<(u32, u32)> = (0..layers.len().saturating_sub(1))
            .into_par_iter()
            .flat_map_iter(|k| {
                let (lower, upper) = (&layers[k], &layers[k + 1]);
                let mut grid: FxHashMap<(i32, i32), Vec<u32>> = FxHashMap::default();
                for (j, section) in upper.iter().enumerate() {
                    for c in section.rect.cells(BLOB_CELL_MM) {
                        grid.entry(c).or_default().push(j as u32);
                    }
                }
                let mut pairs = Vec::new();
                let mut candidates = Vec::new();
                for (i, section) in lower.iter().enumerate() {
                    let r = &section.rect;
                    candidates.clear();
                    let reach = Rect {
                        min: [r.min[0] - LINK_MARGIN_MM, r.min[1] - LINK_MARGIN_MM],
                        max: [r.max[0] + LINK_MARGIN_MM, r.max[1] + LINK_MARGIN_MM],
                    };
                    for c in reach.cells(BLOB_CELL_MM) {
                        candidates.extend(grid.get(&c).into_iter().flatten().copied());
                    }
                    candidates.sort_unstable();
                    candidates.dedup();
                    for &j in &candidates {
                        if r.overlaps(&upper[j as usize].rect, LINK_MARGIN_MM) {
                            pairs.push((offsets[k] + i as u32, offsets[k + 1] + j));
                        }
                    }
                }
                pairs
            })
            .collect();

        let mut up = vec![Vec::new(); blobs.len()];
        let mut down = vec![Vec::new(); blobs.len()];
        for (below, above) in links {
            up[below as usize].push(above);
            down[above as usize].push(below);
        }
        let slopes = vec![Cell::new(f32::NAN); blobs.len()];
        Some(Graph { blobs, up, down, floor: plane(0), step, slopes })
    }

    fn centre(&self, i: u32) -> Vec3 {
        let b = &self.blobs[i as usize];
        Vec3::new(b.x, b.y, b.z)
    }

    /// Follows the shaft down from `start` until the plate, a claimed blob or a dead end.
    ///
    /// Until the walk has come far enough to know the shaft's direction it steers as if the
    /// shaft were upright, so it skips the slices of a tip leaning steeply away from its
    /// contact as drifting. The run of those that each carry on the tip's step from the slice
    /// before (as narrow as the tip, and unclaimed) is kept for fitting the tip. The walk does
    /// not steer by them: the shaft below a tip seldom carries on at the tip's lean. Once the
    /// slices it skips settle into a straight run of one width (the shaft below the tip, or past
    /// a bend), or have done so by the time it gives up skipping, the walk takes the run as the
    /// shaft from where it began, and steers by the run's lean rather than by the nodes above it.
    /// On a leaning shaft it knows the course of, a fork's most vertical way off to the side of
    /// that course gives way to the shaft's own slices carrying on along it, unless an earlier
    /// walk took that way or the shaft's slices soon end. With `pass_beside`, a way at a fork
    /// beside the blob the walk is on is passed up. The walk takes only the blobs `open` lets it.
    fn walk(&self, start: u32, claimed: &[u32], pass_beside: bool, open: impl Fn(u32) -> bool) -> Walk {
        let mut below: Vec<u32> = Vec::new();
        let mut nodes = vec![start];
        let mut visited = vec![start];
        let mut width = self.blobs[start as usize].half_max;
        // Top of the current run of skipped blobs, and where the walk last picked the shaft up
        // again after giving up on one.
        let mut skipped_from: Option<f32> = None;
        let mut picked_up: Option<f32> = None;
        // A leaning tip's skipped slices, where they go among the nodes, the tip's width, and
        // whether the run of them may go on.
        let (mut leaning, mut leaning_at, mut leaning_width, mut leaning_open) = (Vec::new(), 0, width, true);
        // The walk steers by `nodes[from..]`, and by the lean of the run of skipped blobs it last
        // picked up, until those nodes give the shaft's direction; and the current run of
        // skipped blobs that may be the shaft.
        let (mut from, mut lean): (usize, Option<[f32; 2]>) = (0, None);
        let mut run: Vec<u32> = Vec::new();
        let mut current = start;
        loop {
            below.clear();
            below.extend(self.down[current as usize].iter().copied().filter(|&i| open(i)));
            let Some(&first) = below.first() else { break };
            // On a shaft the walk is following, rather than skipping past: it has been on it,
            // without picking it up again after a run of skipped blobs, long enough to know its
            // direction.
            let z = self.blobs[first as usize].z;
            let last = self.blobs[*nodes.last().expect("walk has a start") as usize];
            let course = &nodes[from..];
            let tracking = last.z - z <= DRIFT_LAG_MM
                && picked_up.is_none_or(|top| top - last.z >= DIRECTION_SPAN_MM)
                && self.course(course, lean, z).is_some();
            let next = match below.as_slice() {
                [only] => *only,
                many => {
                    let (px, py) = self.predict(course, lean, z);
                    let miss = |i: u32| {
                        let b = &self.blobs[i as usize];
                        (b.x - px).powi(2) + (b.y - py).powi(2)
                    };
                    // A way beside the blob the walk is on (neither holds the other's middle) is
                    // another support touching it, not where its own shaft goes on.
                    let here = self.blobs[current as usize];
                    let beside = |i: u32| {
                        let b = &self.blobs[i as usize];
                        !(here.rect.holds([b.x, b.y], 0.0) || b.rect.holds([here.x, here.y], 0.0))
                    };
                    let ways: Vec<u32> = if pass_beside && many.iter().any(|&i| !beside(i)) {
                        many.iter().copied().filter(|&i| !beside(i)).collect()
                    } else {
                        many.to_vec()
                    };
                    let slopes: Vec<f32> = ways
                        .iter()
                        .map(|&i| {
                            tracking
                                .then(|| self.merged_descent_slope(i, course, lean, width))
                                .flatten()
                                .unwrap_or_else(|| self.descent_slope(i))
                        })
                        .collect();
                    let steepest = slopes.iter().copied().fold(f32::MAX, f32::min);
                    let upright = ways
                        .iter()
                        .zip(&slopes)
                        .filter(|(_, slope)| **slope <= steepest + SLOPE_TOLERANCE)
                        .map(|(&i, _)| i)
                        .min_by(|&a, &b| miss(a).total_cmp(&miss(b)))
                        .expect("non-empty");
                    // A leaning shaft converging on an upright post it only touches: the post is
                    // the more upright way down, but lies off to the side of where the shaft is
                    // heading, and the shaft's own slices carry on along its course. A post an
                    // earlier walk took is the support the shaft runs into.
                    let on_course = |i: u32| {
                        miss(i).sqrt() <= DRIFT_MM + width * DRIFT_RATIO
                            && self.blobs[i as usize].half_max <= width * WIDEN_RATIO + WIDEN_SLACK_MM
                            && self.runs_on(i, DIRECTION_SPAN_MM)
                    };
                    let leans = (px - last.x).hypot(py - last.y) >= MIN_COURSE_LEAN * (last.z - z);
                    if tracking && leans && claimed[upright as usize] == NONE && !on_course(upright) {
                        ways.iter()
                            .copied()
                            .filter(|&i| on_course(i))
                            .min_by(|&a, &b| miss(a).total_cmp(&miss(b)))
                            .unwrap_or(upright)
                    } else {
                        upright
                    }
                }
            };
            visited.push(next);
            let blob = &self.blobs[next as usize];
            let last_z = self.blobs[*nodes.last().expect("walk has a start") as usize].z;
            let settled = nodes[from..]
                .iter()
                .rposition(|&i| self.blobs[i as usize].z >= last_z + DRIFT_LAG_MM)
                .map(|s| from + s);
            let (px, py) = self.predict(&nodes[from..=settled.unwrap_or(nodes.len() - 1)], lean, blob.z);
            let drift = ((blob.x - px).powi(2) + (blob.y - py).powi(2)).sqrt();
            let narrow = blob.half_max <= width * WIDEN_RATIO + WIDEN_SLACK_MM;
            // On a shaft the walk is following, a blob wider than it is a joint ball centred on
            // it, or the shaft merged with a brace or crossing bar, centred off the shaft.
            let merged = tracking && blob.half_max > width * SHAFT_ALONE_RATIO + SHAFT_ALONE_SLACK_MM;
            let aligned = drift <= DRIFT_MM + if merged { 0.0 } else { width * DRIFT_RATIO };
            let reset =
                blob.half_max <= MAX_RESET_WIDTH_MM && skipped_from.is_some_and(|top| top - blob.z >= SKIP_RESET_MM);
            // Where the walk gives up skipping, the slices it skipped may already have settled into
            // a straight run of one width that this one carries on: that run is the shaft.
            let run_ends = reset
                && !(narrow && aligned)
                && blob.z >= ROOT_TOP_MM
                && run.len() >= RESET_RUN_SLICES
                && self.carries_run(&run, next);
            if reset {
                width = width.max(blob.half_max);
            }
            if ((narrow && aligned) || reset) && !run_ends {
                let owner = claimed[next as usize];
                if owner != NONE {
                    let end = WalkEnd::Joined(owner as usize, next);
                    return Walk { nodes, leaning, leaning_at, visited, end };
                }
                let step = (self.centre(next) - self.centre(*nodes.last().expect("walk has a start"))).length();
                width += ((blob.half_max - width) * WIDTH_SMOOTHING).min(MAX_WIDTH_GROWTH * step);
                if !(narrow && aligned) {
                    picked_up = Some(blob.z);
                }
                skipped_from = None;
                leaning_open &= leaning.is_empty();
                run.clear();
                nodes.push(next);
            } else {
                if run_ends {
                    run.push(next);
                } else {
                    skipped_from.get_or_insert(blob.z);
                    let mut tip_slice = false;
                    if leaning_open {
                        if leaning.is_empty() {
                            (leaning_at, leaning_width) = (nodes.len(), width);
                        }
                        // Where the tip's last step from slice to slice carries it on to.
                        let mut tip = nodes[..leaning_at].iter().chain(&leaning).rev().map(|&i| self.centre(i));
                        let previous = tip.next().expect("walk has a start");
                        let carried = previous + (previous - tip.next().unwrap_or(previous));
                        let steered = settled.is_some_and(|s| self.heading(&nodes[..=s], blob.z).is_some());
                        leaning_open = !steered
                            && claimed[next as usize] == NONE
                            && blob.half_max <= leaning_width * WIDEN_RATIO + WIDEN_SLACK_MM
                            && (blob.x - carried.x).hypot(blob.y - carried.y) <= DRIFT_MM + leaning_width * DRIFT_RATIO;
                        if leaning_open {
                            let step = (self.centre(next) - previous).length();
                            leaning_width +=
                                ((blob.half_max - leaning_width) * WIDTH_SMOOTHING).min(MAX_WIDTH_GROWTH * step);
                            leaning.push(next);
                            tip_slice = true;
                        }
                    }
                    if tip_slice || !self.carries_run(&run, next) {
                        run.clear();
                    }
                    if !tip_slice && blob.half_max <= MAX_RESET_WIDTH_MM {
                        run.push(next);
                    }
                }
                let top = run.first().map_or(blob.z, |&i| self.blobs[i as usize].z);
                if run_ends || top - blob.z >= PICKUP_RUN_MM {
                    // A shaft: picked up from where the run began, steering by its lean.
                    for &i in &run {
                        let owner = claimed[i as usize];
                        if owner != NONE {
                            let end = WalkEnd::Joined(owner as usize, i);
                            return Walk { nodes, leaning, leaning_at, visited, end };
                        }
                        nodes.push(i);
                    }
                    let (a, b) = (self.centre(run[0]), self.centre(next));
                    let height = a.z - b.z;
                    lean = Some([(b.x - a.x) / height, (b.y - a.y) / height]);
                    from = nodes.len() - run.len();
                    let run_width =
                        run.iter().map(|&i| self.blobs[i as usize].half_max).sum::<f32>() / run.len() as f32;
                    width = width.max(run_width);
                    picked_up = None;
                    skipped_from = None;
                    leaning_open &= leaning.is_empty();
                    run.clear();
                }
            }
            current = next;
        }
        let end = if self.floor <= PLATE_REACH_MM && self.blobs[current as usize].z <= self.floor + PLATE_REACH_MM {
            WalkEnd::Plate
        } else {
            WalkEnd::Stranded
        };
        Walk { nodes, leaning, leaning_at, visited, end }
    }

    /// Whether the structure below `start` reaches `depth` further down, or the plate.
    fn runs_on(&self, start: u32, depth: f32) -> bool {
        let top = self.blobs[start as usize].z;
        let bottom = (top - depth).max(self.floor + PLATE_REACH_MM);
        let mut layer = vec![start];
        while let Some(&first) = layer.first() {
            if self.blobs[first as usize].z <= bottom {
                return true;
            }
            let mut below: Vec<u32> = layer.iter().flat_map(|&i| self.down[i as usize].iter().copied()).collect();
            below.sort_unstable();
            below.dedup();
            below.truncate(FORK_WAYS);
            layer = below;
        }
        false
    }

    /// Horizontal run per unit of height of the most vertical way down from `start`: to the blob
    /// [`FORK_LOOKAHEAD_MM`] below (or where the structure ends, if sooner) nearest the start's
    /// line, among those reachable from it. Only where a way ends counts, so a joint or brace
    /// junction on the way, whose slices sit off the shaft's line, does not make a post read as
    /// leaning.
    fn descent_slope(&self, start: u32) -> f32 {
        let cached = self.slopes[start as usize].get();
        if !cached.is_nan() {
            return cached;
        }
        let slope = self.measure_descent_slope(start);
        self.slopes[start as usize].set(slope);
        slope
    }

    fn measure_descent_slope(&self, start: u32) -> f32 {
        let top = self.blobs[start as usize];
        let off = |i: u32| {
            let b = &self.blobs[i as usize];
            (b.x - top.x).hypot(b.y - top.y)
        };
        // Roots, rafts and base plates below the root top lie wide around the supports they join,
        // so they tell nothing of a way's direction: a way ends at the root top, and below it any
        // way reaching the plate is as good as another.
        let root_zone = self.floor <= PLATE_REACH_MM;
        let mut layer = vec![start];
        loop {
            let z = self.blobs[layer[0] as usize].z;
            let mut below: Vec<u32> = layer.iter().flat_map(|&i| self.down[i as usize].iter().copied()).collect();
            if root_zone && top.z < ROOT_TOP_MM {
                if below.is_empty() {
                    return if z <= self.floor + PLATE_REACH_MM { 0.0 } else { f32::MAX };
                }
            } else if top.z - z >= FORK_LOOKAHEAD_MM || below.is_empty() || (root_zone && z - self.step < ROOT_TOP_MM) {
                let drop = top.z - z;
                if drop <= 0.0 {
                    return f32::MAX;
                }
                return layer.iter().map(|&i| off(i)).fold(f32::MAX, f32::min) / drop;
            }
            below.sort_unstable();
            below.dedup();
            below.sort_by(|&a, &b| off(a).total_cmp(&off(b)));
            below.truncate(FORK_WAYS);
            layer = below;
        }
    }

    /// When fork blob `start` is the shaft the walk follows through `nodes` (top down), of
    /// half-width `width`, merged with a brace or crossing bar beside it (wider than the shaft,
    /// holding where the walk is heading), the [`Graph::descent_slope`] of the shaft running on
    /// from it: the blob's middle lies off the shaft's line, so the way is followed from where
    /// the walk is heading, down the shaft's line, to the last blob as wide as the shaft.
    fn merged_descent_slope(&self, start: u32, nodes: &[u32], lean: Option<[f32; 2]>, width: f32) -> Option<f32> {
        let top = self.blobs[start as usize];
        let (px, py) = self.predict(nodes, lean, top.z);
        let merged = top.half_max > width * MERGED_RATIO + MERGED_SLACK_MM
            && top.half_max <= width * WIDEN_RATIO + WIDEN_SLACK_MM
            && top.z >= ROOT_TOP_MM
            && top.rect.holds([px, py], 0.0);
        if !merged {
            return None;
        }
        let origin = Vec3::new(px, py, top.z);
        let mut on_shaft: Option<Vec3> = None;
        let mut current = start;
        loop {
            let here = self.blobs[current as usize];
            let below = &self.down[current as usize];
            let Some(&first) = below.first() else { break };
            if top.z - here.z >= FORK_LOOKAHEAD_MM {
                break;
            }
            let z = self.blobs[first as usize].z;
            let (qx, qy) = match on_shaft {
                Some(a) => {
                    let t = (origin.z - z) / (origin.z - a.z);
                    (origin.x + (a.x - origin.x) * t, origin.y + (a.y - origin.y) * t)
                }
                None => self.predict(nodes, lean, z),
            };
            current = below
                .iter()
                .copied()
                .min_by(|&a, &b| {
                    let d = |i: u32| {
                        let b = &self.blobs[i as usize];
                        (b.x - qx).powi(2) + (b.y - qy).powi(2)
                    };
                    d(a).total_cmp(&d(b))
                })
                .expect("non-empty");
            if self.blobs[current as usize].half_max <= width * SHAFT_ALONE_RATIO + SHAFT_ALONE_SLACK_MM {
                on_shaft = Some(self.centre(current));
            }
        }
        let end = on_shaft.unwrap_or(self.centre(current));
        let drop = origin.z - end.z;
        if current == start || drop <= 0.0 {
            return Some(f32::MAX);
        }
        Some((end.x - origin.x).hypot(end.y - origin.y) / drop)
    }

    /// Support geometry no walk passed, grouped into connected pieces, each with the places
    /// it touches traced supports (or posts) as candidate brace and kickstand ends. Owners
    /// below `hosts` are supports and posts; an owner from `hosts` on is a kickstand:
    /// `kickstands[owner - hosts]`.
    fn untraced(&self, seen: &[bool], owner: &[u32], hosts: usize, kickstands: &[KickstandLayout]) -> Vec<Piece> {
        let n = self.blobs.len();
        let mut parent: Vec<u32> = (0..n as u32).collect();
        for i in (0..n).filter(|&i| !seen[i]) {
            for &j in self.up[i].iter().filter(|&&j| !seen[j as usize]) {
                union(&mut parent, i as u32, j);
            }
        }
        let mut pieces: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
        for i in (0..n).filter(|&i| !seen[i]) {
            pieces.entry(find(&mut parent, i as u32)).or_default().push(i as u32);
        }
        let mut pieces: Vec<Vec<u32>> = pieces.into_values().filter(|p| p.len() >= MIN_BRACE_BLOBS).collect();
        pieces.sort_by_key(|p| p[0]);

        pieces
            .into_iter()
            .map(|blobs| {
                // Where the piece touches traced supports, grouped per support by height.
                let mut touches: Vec<(u32, Vec3)> = blobs
                    .iter()
                    .flat_map(|&b| self.up[b as usize].iter().chain(&self.down[b as usize]))
                    .filter(|&&other| owner[other as usize] != NONE)
                    .map(|&other| (owner[other as usize], self.centre(other)))
                    .collect();
                touches.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.z.total_cmp(&b.1.z)));
                let mut ends: Vec<BraceEnd> = Vec::new();
                let mut grouped: Vec<Vec<Vec3>> = Vec::new();
                for group in touches.chunk_by(|a, b| a.0 == b.0 && b.1.z - a.1.z <= BRACE_END_GAP_MM) {
                    let sum = group.iter().fold(Vec3::default(), |sum, t| sum + t.1);
                    let point = sum * (1.0 / group.len() as f32);
                    let id = group[0].0 as usize;
                    ends.push(match id.checked_sub(hosts) {
                        Some(k) => BraceEnd { support: kickstands[k].support, kickstand: Some(k), point },
                        None => BraceEnd { support: id, kickstand: None, point },
                    });
                    grouped.push(group.iter().map(|t| t.1).collect());
                }
                let mut cells: FxHashMap<(u32, i32, i32), Vec<u32>> = FxHashMap::default();
                for &b in &blobs {
                    let blob = &self.blobs[b as usize];
                    let r = &blob.rect;
                    let reach = Rect {
                        min: [r.min[0] - BRACE_MARGIN_MM, r.min[1] - BRACE_MARGIN_MM],
                        max: [r.max[0] + BRACE_MARGIN_MM, r.max[1] + BRACE_MARGIN_MM],
                    };
                    for (x, y) in reach.cells(BLOB_CELL_MM) {
                        cells.entry((blob.layer, x, y)).or_default().push(b);
                    }
                }
                Piece { blobs, ends, touches: grouped, cells }
            })
            .collect()
    }

    /// Braces: untraced pieces that bridge two traced supports. Two of a piece's ends on
    /// different supports are braced when the piece's own cross-sections cover the straight
    /// line between them, which tells an X of crossing braces, or a V meeting on one post, from
    /// the lines its ends do not run along. Where both braces of an X meet each support in one
    /// junction, they are found from its lower and upper parts (see [`Graph::crossing`]). With
    /// `on_kickstands`, only braces with an end on a kickstand are added to `braces`.
    fn braces(
        &self,
        pieces: &[Piece],
        shafts: &[Shaft],
        shaft_hosts: &[(Host, bool)],
        braces: &mut Vec<BraceLayout>,
        on_kickstands: bool,
    ) {
        for piece in pieces {
            let ends = &piece.ends;
            // Every pair of ends on different supports that the piece runs between, and for the
            // two braces of an X meeting each support in one junction, the points each runs
            // between.
            let mut found = Vec::new();
            for (i, a) in ends.iter().enumerate() {
                for (j, b) in ends.iter().enumerate().skip(i + 1) {
                    if a.same_host(b)
                        || (on_kickstands && a.kickstand.is_none() && b.kickstand.is_none())
                        || (b.point - a.point).length() < MIN_BRACE_MIN_MM
                        || a.point.z.min(b.point.z) < ROOT_TOP_MM
                    {
                        continue;
                    }
                    let Some(diameter_mm) = self.brace_along(piece, a.point, b.point) else { continue };
                    if (b.point - a.point).length() < min_brace_mm(diameter_mm) {
                        continue;
                    }
                    match self.crossing(piece, i, j, diameter_mm) {
                        Some([(p, q, d), (r, s, e)]) => {
                            found.push((i, j, Some((p, q)), d));
                            found.push((i, j, Some((r, s)), e));
                        }
                        None => found.push((i, j, None, diameter_mm)),
                    }
                }
            }
            let braced = |i: usize, j: usize| found.iter().any(|&(x, y, _, _)| (x, y) == (i.min(j), i.max(j)));
            for &(i, j, crossed, diameter_mm) in &found {
                let (a, b) = match crossed {
                    Some((p, q)) => (BraceEnd { point: p, ..ends[i] }, BraceEnd { point: q, ..ends[j] }),
                    None => (ends[i], ends[j]),
                };
                let through = crossed.is_none()
                    && ends.iter().enumerate().any(|(k, e)| {
                        !e.same_host(&a)
                            && !e.same_host(&b)
                            && distance_to_segment(e.point, a.point, b.point) < BRACE_THROUGH_MM
                            && braced(i, k)
                            && braced(k, j)
                    });
                if through {
                    continue;
                }
                let (lower, upper) = if a.point.z <= b.point.z { (a, b) } else { (b, a) };
                // A line fitted along one brace of an X would take in the other where they cross.
                let (lower, upper) = match crossed {
                    Some(_) => (lower, upper),
                    None => self.refit_brace(&piece.blobs, lower, upper, diameter_mm, shafts, shaft_hosts),
                };
                if lower.point.z < ROOT_TOP_MM {
                    continue;
                }
                // Supports and kickstands meeting at one hub each touch a brace leaving it.
                let same = |e: &BraceEnd, f: &BraceEnd| (e.point - f.point).length() < DUPLICATE_BRACE_MM;
                if braces.iter().any(|x| same(&x.lower, &lower) && same(&x.upper, &upper)) {
                    continue;
                }
                let hosts = [(lower.support, lower.kickstand), (upper.support, upper.kickstand)];
                let knot_reach = knot_reach_mm(diameter_mm);
                // A knot hangs on a shaft: an end on a tip only, or off its support, has none.
                // A branch hanging down from where it joins its parent to its socket (a fan of tip
                // cones meeting in one joint ball below that) has no shaft of its own either: the
                // knots of braces on such branches all come together at the joint.
                let knotted = |end: &BraceEnd, host: Host| {
                    let shaft: Vec<&Shaft> = shafts
                        .iter()
                        .zip(shaft_hosts)
                        .filter(|&(_, &(h, knotted))| knotted && h == host)
                        .map(|(s, _)| s)
                        .collect();
                    let hangs = shaft.first().zip(shaft.last()).is_some_and(|(first, last)| first.0.z > last.1.z);
                    !hangs
                        && shaft
                            .iter()
                            .any(|&&(a, b, radius)| distance_to_segment(end.point, a, b) <= radius + knot_reach)
                };
                if !knotted(&lower, hosts[0]) || !knotted(&upper, hosts[1]) {
                    continue;
                }
                // Posts packed close together, as kickstands, each touch the slices merging them:
                // a brace from one running on through the next is not there.
                if along_shaft(shafts, lower.point, upper.point, diameter_mm)
                    || (on_kickstands && through_shaft(shafts, shaft_hosts, hosts, lower.point, upper.point))
                {
                    continue;
                }
                log::trace!(
                    "Brace from support {} at {} to support {} at {}, {:.2} mm across",
                    lower.support,
                    lower.point,
                    upper.support,
                    upper.point,
                    diameter_mm
                );
                braces.push(BraceLayout { lower, upper, diameter_mm });
            }
        }
    }

    /// The brace's ends moved onto a line fitted through the piece's cross-sections between
    /// them, each where that line passes closest to its own shaft (one of `shafts`, which belong
    /// to `shaft_hosts`), short of its tip. An end found from where the piece touches a support
    /// sits mid-way along the junction, which flattens a diagonal.
    fn refit_brace(
        &self,
        piece: &[u32],
        lower: BraceEnd,
        upper: BraceEnd,
        diameter_mm: f32,
        shafts: &[Shaft],
        shaft_hosts: &[(Host, bool)],
    ) -> (BraceEnd, BraceEnd) {
        let reach = min_brace_mm(diameter_mm);
        let Some((mean, direction)) = self.fit_line(piece, lower.point, upper.point, reach / 2.0) else {
            return (lower, upper);
        };
        let on_line = |p: Vec3| mean + direction * (p - mean).dot(direction);
        let onto = |end: BraceEnd| {
            let host = (end.support, end.kickstand);
            let crossing = shafts
                .iter()
                .zip(shaft_hosts)
                .filter(|&(_, &(h, knotted))| knotted && h == host)
                .map(|(&(a, b, _), _)| on_line(closest_to_line(a, b, mean, direction)))
                .filter(|p| (*p - end.point).length() <= reach)
                .min_by(|p, q| (*p - end.point).length_sq().total_cmp(&(*q - end.point).length_sq()));
            BraceEnd { point: crossing.unwrap_or_else(|| on_line(end.point)), ..end }
        };
        let (a, b) = (onto(lower), onto(upper));
        // A line fitted across the brace (through a crossing brace) rather than along it would
        // shrink it to a stub.
        if (b.point - a.point).length() < (upper.point - lower.point).length() * BRACE_REFIT_SPREAD {
            return (lower, upper);
        }
        if a.point.z <= b.point.z { (a, b) } else { (b, a) }
    }

    /// A line (a point on it and its direction) fitted through the piece's cross-sections
    /// within `radius` of the line from `p` to `q`, leaving out the outer [`BRACE_END_SKIP`] at
    /// each end. None when they do not spread along at least [`BRACE_REFIT_SPREAD`] of it.
    fn fit_line(&self, piece: &[u32], p: Vec3, q: Vec3, radius: f32) -> Option<(Vec3, Vec3)> {
        let axis = q - p;
        let length_sq = axis.length_sq();
        let along: Vec<Vec3> = piece
            .iter()
            .map(|&b| self.centre(b))
            .filter(|c| {
                let t = (*c - p).dot(axis) / length_sq;
                (BRACE_END_SKIP..=1.0 - BRACE_END_SKIP).contains(&t) && distance_to_segment(*c, p, q) < radius
            })
            .collect();
        if along.len() < MIN_BRACE_BLOBS {
            return None;
        }
        let mean = along.iter().fold(Vec3::default(), |sum, c| sum + *c) * (1.0 / along.len() as f32);
        // Principal direction by power iteration, starting from the end-to-end direction.
        let mut direction = axis.normalized();
        for _ in 0..20 {
            let mut next = Vec3::default();
            for c in &along {
                let d = *c - mean;
                next = next + d * d.dot(direction);
            }
            if next.length_sq() < 1e-12 {
                return None;
            }
            direction = next.normalized();
        }
        let (lo, hi) = along
            .iter()
            .map(|c| (*c - mean).dot(direction))
            .fold((f32::MAX, f32::MIN), |(l, h), t| (l.min(t), h.max(t)));
        (hi - lo >= axis.length() * BRACE_REFIT_SPREAD).then_some((mean, direction))
    }

    /// The bottoms of a piece below the root top with nothing untraced below them (on the
    /// plate, a raft or a root): where kickstands and posts stand.
    fn feet<'a>(&'a self, piece: &'a Piece, seen: &'a [bool]) -> impl Iterator<Item = u32> + 'a {
        piece.blobs.iter().copied().filter(move |&b| {
            self.blobs[b as usize].z < ROOT_TOP_MM && self.down[b as usize].iter().all(|&below| seen[below as usize])
        })
    }

    /// The diameter of the leg a piece forms from `start` up to `top`, when it is long enough,
    /// leans as a kickstand does and the piece's cross-sections cover it.
    fn leg_along(&self, piece: &Piece, start: Vec3, top: Vec3) -> Option<f32> {
        let span = top - start;
        let lean = (span.x * span.x + span.y * span.y).sqrt() / span.z;
        if span.length() < MIN_KICKSTAND_MM || !(MIN_KICKSTAND_LEAN..=MAX_KICKSTAND_LEAN).contains(&lean) {
            return None;
        }
        self.brace_along(piece, start, top)
    }

    /// Posts: columns of untraced geometry standing on the plate that legs lean against,
    /// splaying from them down to the plate (a lattice of posts with legs crossing between
    /// them). DragonFruit cannot hang a kickstand from a kickstand, so such a column stands as
    /// a post of its own, from its root up to its top or on into the trunk or branch (one of
    /// `hosts`) right above it, and its legs are kickstands holding it up; a strut leaning off
    /// it onto a traced support is a brace. A leg runs from another foot of the piece up to
    /// where struts meet the column, or its top, and the piece covers it. So does a column
    /// standing free, braced to traced supports but holding none up (it does not run into one,
    /// and no strut leans off it onto one): its braces hang from it.
    fn posts(&self, pieces: &[Piece], seen: &[bool], owner: &[u32], hosts: &[HostLine]) -> Vec<(PostLayout, usize)> {
        let mut posts: Vec<(PostLayout, usize)> = Vec::new();
        if self.floor > PLATE_REACH_MM {
            return posts;
        }
        let (shafts, _) = traced_shafts(hosts);
        for (p, piece) in pieces.iter().enumerate() {
            let feet: Vec<u32> = self.feet(piece, seen).collect();
            // Where the piece touches a trunk or branch a strut could lean onto, and whether it
            // touches any traced support above the root top.
            let tops: Vec<Vec3> = piece
                .ends
                .iter()
                .zip(&piece.touches)
                .filter(|(end, _)| !hosts[end.support].strut)
                .flat_map(|(_, touches)| touches.iter().copied().filter(|t| t.z >= ROOT_TOP_MM))
                .collect();
            let braced = piece.ends.iter().any(|end| end.point.z >= ROOT_TOP_MM);
            for &foot in &feet {
                let Some(column) = self.column(seen, owner, foot) else { continue };
                let starts: Vec<Vec3> = column.junctions.iter().copied().chain([column.top]).collect();
                let leaned_on = feet.iter().any(|&leg| {
                    let start = self.centre(leg);
                    starts.iter().any(|&at| self.leg_along(piece, start, at).is_some())
                });
                let end = self.column_end(&column, hosts);
                let free = || {
                    braced
                        && end.is_none()
                        && !starts
                            .iter()
                            .any(|&start| tops.iter().any(|&top| self.leg_along(piece, start, top).is_some()))
                };
                let base = Vec3::new(column.top.x, column.top.y, ROOT_TOP_MM);
                let diameter_mm = shaft_diameter(column.widths.clone(), column.radius * 2.0);
                if !(leaned_on || free())
                    || posts.iter().any(|(post, _)| (post.foot - base).length() < DUPLICATE_BRACE_MM)
                    || along_shaft(&shafts, base, column.top, diameter_mm)
                {
                    continue;
                }
                let (joints, top) = match end {
                    Some((_, end, joints)) => (joints, end),
                    None => (Vec::new(), column.top),
                };
                log::trace!("Post from {base} to {top} ({} joints), {diameter_mm:.2} mm across", joints.len());
                let root_diameter_mm =
                    (diameter_mm * ROOT_DIAMETER_RATIO).clamp(MIN_ROOT_DIAMETER_MM, MAX_ROOT_DIAMETER_MM);
                posts.push((PostLayout { foot: base, joints, top, root_diameter_mm, diameter_mm }, p));
            }
        }
        posts
    }

    /// Kickstands: untraced pieces standing on the plate that lean against a trunk or branch.
    /// Each bottom of a piece below the root top (nothing untraced below it, so the plate, a
    /// raft or a root) is a candidate foot, and each place the piece touches a trunk or branch
    /// above the root top a candidate knot. A foot and a knot are joined by a leg when the
    /// piece's own cross-sections cover the line between them, as a brace's do, or the line to
    /// the knot from a column rising straight up from the foot: from its top (a post with a strut
    /// leaning from its top, the shape DragonFruit gives its own kickstands) or from a junction
    /// lower down. A post running straight up into a trunk or branch holds it up too. `hosts`
    /// are the traced supports and posts a kickstand can hold up.
    fn kickstands(
        &self,
        pieces: &[Piece],
        seen: &[bool],
        owner: &[u32],
        hosts: &[HostLine],
        shafts: &[Shaft],
    ) -> Vec<(KickstandLayout, usize)> {
        let mut kickstands: Vec<(KickstandLayout, usize)> = Vec::new();
        if self.floor > PLATE_REACH_MM {
            return kickstands;
        }
        for (p, piece) in pieces.iter().enumerate() {
            let feet: Vec<(Vec3, Option<Column>)> =
                self.feet(piece, seen).map(|b| (self.centre(b), self.column(seen, owner, b))).collect();
            if feet.is_empty() {
                continue;
            }
            // Each kickstand found, and the top of its column when it stands on one.
            let mut found: Vec<(KickstandLayout, Option<f32>)> = Vec::new();
            // A post running up into a trunk or branch holds it up from below.
            for column in feet.iter().filter_map(|(_, c)| c.as_ref()) {
                found.extend(
                    self.post_under(column, hosts, shafts).map(|k| (k, Some(column.into.map_or(0.0, |i| i.1.z)))),
                );
            }
            for (&mean, touches) in piece.ends.iter().zip(&piece.touches) {
                if hosts[mean.support].strut {
                    continue;
                }
                // Where along the support the strut meets it: the touches run the length of the
                // junction, and further when other struts and braces meet the support next to it.
                let mut tops: Vec<Vec3> = Vec::new();
                for &touch in touches {
                    if touch.z >= ROOT_TOP_MM && tops.last().is_none_or(|last| touch.z - last.z >= KNOT_STEP_MM) {
                        tops.push(touch);
                    }
                }
                tops.sort_by(|a, b| (a.z - mean.point.z).abs().total_cmp(&(b.z - mean.point.z).abs()));
                for (foot, column) in &feet {
                    // The strut runs from the foot itself, or else from the column: its top, or
                    // where the strut leaves it lower down.
                    let column_starts =
                        column.iter().flat_map(|c| std::iter::once(c.top).chain(c.junctions.iter().copied()));
                    let starts = std::iter::once(*foot).chain(column_starts);
                    let leg =
                        starts.flat_map(|start| tops.iter().map(move |&top| (start, top))).find_map(|(start, top)| {
                            let diameter_mm = self.leg_along(piece, start, top)?;
                            let end = BraceEnd { point: top, ..mean };
                            if self.runs_through(piece, hosts, start, end, diameter_mm) {
                                return None;
                            }
                            let column = column.as_ref().filter(|_| start != *foot).map(|c| c.top);
                            let leg = self.fit_leg(piece, hosts, end, start, column, diameter_mm, shafts)?;
                            // The fitted strut too: a foot off to the side of the strut it stands
                            // under can hide that it runs on through another support.
                            let bottom = leg.joints.last().copied().unwrap_or(leg.foot);
                            let end = BraceEnd { point: leg.knot, ..end };
                            (!self.runs_through(piece, hosts, bottom, end, diameter_mm)).then_some(leg)
                        });
                    found.extend(leg.map(|k| {
                        let top = k.joints.last().map(|j| j.z);
                        (k, top)
                    }));
                }
            }
            // A post holds up one kickstand, the one its column rises highest in; struts leaving
            // it lower down are braces from that kickstand.
            found.sort_by(|a, b| b.1.unwrap_or(f32::MIN).total_cmp(&a.1.unwrap_or(f32::MIN)));
            let mut posts: Vec<Vec3> = Vec::new();
            for (kickstand, column) in found {
                if column.is_some() {
                    if posts.iter().any(|p| (*p - kickstand.foot).length() < DUPLICATE_BRACE_MM) {
                        continue;
                    }
                    posts.push(kickstand.foot);
                }
                // Supports meeting at one hub each touch a leg leaving it: it is one leg.
                let same = |(k, _): &(KickstandLayout, usize)| {
                    (k.knot - kickstand.knot).length() < DUPLICATE_BRACE_MM
                        && (k.foot - kickstand.foot).length() < DUPLICATE_BRACE_MM
                };
                if kickstands.iter().any(same) {
                    continue;
                }
                log::trace!(
                    "Kickstand from {} to support {} at {} ({} joints), {:.2} mm across",
                    kickstand.foot,
                    kickstand.support,
                    kickstand.knot,
                    kickstand.joints.len(),
                    kickstand.diameter_mm
                );
                kickstands.push((kickstand, p));
            }
        }
        kickstands
    }

    /// The column rising straight up from foot blob `foot` (a post), when it reaches at least
    /// [`MIN_KICKSTAND_MM`] above the root top. It is followed up its own centre line through
    /// the junctions where struts and braces merge into it or meet it, as far as any untraced
    /// slice holds that line.
    fn column(&self, seen: &[bool], owner: &[u32], foot: u32) -> Option<Column> {
        let base = self.blobs[foot as usize];
        let mut centre = [base.x, base.y];
        let (mut sum, mut count) = ([0.0f32; 2], 0usize);
        let drift = |i: u32, centre: [f32; 2]| {
            let b = &self.blobs[i as usize];
            (b.x - centre[0]).hypot(b.y - centre[1])
        };
        let mut radius: Option<f32> = None;
        let mut top: Option<f32> = None;
        // The bottom of the junction the column is passing through, if it is.
        let mut merged_from: Option<f32> = None;
        let mut junctions = Vec::new();
        let mut widths = Vec::new();
        let mut current = foot;
        let into = loop {
            let on_line = |i: u32| self.blobs[i as usize].rect.holds(centre, 0.0);
            let nearest = |a: &u32, b: &u32| drift(*a, centre).total_cmp(&drift(*b, centre));
            let up = &self.up[current as usize];
            let traced = up.iter().copied().filter(|&i| owner[i as usize] != NONE && on_line(i)).min_by(nearest);
            // A junction a traced support's walk passed through (a brace from the column merging
            // with the support beside it) cuts the column's slices; it carries on above.
            let past_junction = || {
                let mut at = traced?;
                let from = self.blobs[at as usize].z;
                while self.blobs[at as usize].z < from + COLUMN_JUNCTION_MM {
                    let ups = &self.up[at as usize];
                    let own = |i: &u32| {
                        let b = &self.blobs[*i as usize];
                        !seen[*i as usize]
                            && on_line(*i)
                            && drift(*i, centre) <= DRIFT_MM
                            && radius.is_some_and(|r| b.half_max <= r * MERGED_RATIO + MERGED_SLACK_MM)
                    };
                    if let Some(next) = ups.iter().copied().filter(own).min_by(nearest) {
                        return Some(next);
                    }
                    at = ups.iter().copied().filter(|&i| seen[i as usize] && on_line(i)).min_by(nearest)?;
                }
                None
            };
            let next = match up.iter().copied().filter(|&i| !seen[i as usize] && on_line(i)).min_by(nearest) {
                Some(next) => next,
                None => match past_junction() {
                    Some(next) => {
                        merged_from.get_or_insert(self.blobs[current as usize].z);
                        next
                    }
                    None => break traced.map(|i| (owner[i as usize] as usize, self.blobs[i as usize].z)),
                },
            };
            let b = self.blobs[next as usize];
            if b.z > ROOT_TOP_MM {
                let own = drift(next, centre) <= DRIFT_MM
                    && radius.is_none_or(|r| b.half_max <= r * MERGED_RATIO + MERGED_SLACK_MM);
                if own {
                    let r = *radius.get_or_insert(b.half_max);
                    // A strut or brace modelled as a shape of its own meets the column beside it.
                    let touched = self.up[next as usize]
                        .iter()
                        .chain(&self.down[next as usize])
                        .any(|&i| i != current && !seen[i as usize] && drift(i, centre) > r);
                    if touched {
                        merged_from.get_or_insert(b.z);
                    } else if let (Some(from), Some(_)) = (merged_from.take(), top) {
                        junctions.push((from + b.z) / 2.0);
                    }
                    top = Some(b.z);
                    widths.push(b.half_min * 2.0);
                    sum = [sum[0] + b.x, sum[1] + b.y];
                    count += 1;
                    centre = [sum[0] / count as f32, sum[1] / count as f32];
                } else {
                    merged_from.get_or_insert(b.z);
                }
            }
            current = next;
        };
        let (top, radius) = (top?, radius?);
        if top < ROOT_TOP_MM + MIN_KICKSTAND_MM {
            return None;
        }
        let at = |z: f32| Vec3::new(centre[0], centre[1], z);
        Some(Column {
            top: at(top),
            junctions: junctions.into_iter().rev().map(at).collect(),
            into: into.map(|(support, z)| (support, at(z))),
            radius,
            widths,
        })
    }

    /// The slices of `piece` a kickstand or post rebuilt from it passes through, along `line`
    /// from its foot (at the root top) up, its root included.
    fn along(&self, piece: &Piece, line: &[Vec3]) -> Vec<u32> {
        let foot = line[0];
        let line: Vec<Vec3> =
            std::iter::once(Vec3::new(foot.x, foot.y, self.floor)).chain(line.iter().copied()).collect();
        let mut found = Vec::new();
        for w in line.windows(2) {
            let (a, b) = (w[0], w[1]);
            if b.z - a.z < self.step / 2.0 {
                continue;
            }
            let first = ((a.z - self.floor) / self.step).round().max(0.0) as u32;
            let last = ((b.z - self.floor) / self.step).round().max(0.0) as u32;
            for layer in first..=last {
                let z = self.floor + layer as f32 * self.step;
                let p = a + (b - a) * ((z - a.z) / (b.z - a.z)).clamp(0.0, 1.0);
                let (x, y) = cell2([p.x, p.y], BLOB_CELL_MM);
                for &i in piece.cells.get(&(layer, x, y)).into_iter().flatten() {
                    if self.blobs[i as usize].rect.holds([p.x, p.y], 0.0) {
                        found.push(i);
                    }
                }
            }
        }
        found
    }

    /// Where a column runs up into the shaft of a trunk or branch (one of `hosts`) right above
    /// its top: the support, the point on its shaft, and the joint at the column's top on the
    /// way there when the shaft lies off to the side.
    fn column_end(&self, column: &Column, hosts: &[HostLine]) -> Option<(usize, Vec3, Vec<Vec3>)> {
        let (support, at) = column.into?;
        let host = &hosts[support];
        if host.strut {
            return None;
        }
        let knot = host
            .line
            .windows(2)
            .map(|w| {
                let ab = w[1] - w[0];
                let t = if ab.length_sq() > 0.0 { ((at - w[0]).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
                w[0] + ab * t
            })
            .min_by(|a, b| (*a - at).length_sq().total_cmp(&(*b - at).length_sq()))?;
        if (knot - at).length() > host.radius + column.radius + BRACE_MARGIN_MM {
            return None;
        }
        let off_axis = (knot.x - at.x).hypot(knot.y - at.y) > host.radius;
        let joints = if off_axis { vec![column.top] } else { Vec::new() };
        let bottom = joints.last().map_or(ROOT_TOP_MM, |j| j.z);
        (knot.z >= bottom + MIN_SHAFT_LENGTH_MM).then_some((support, knot, joints))
    }

    /// The kickstand a post forms when it runs up into the shaft of a trunk or branch (one of
    /// `hosts`) right above its top: its column stands on its own root and rises straight to a
    /// knot on that shaft, by way of a joint at the column's top when the shaft lies off to the
    /// side.
    fn post_under(&self, column: &Column, hosts: &[HostLine], shafts: &[Shaft]) -> Option<KickstandLayout> {
        let (support, knot, joints) = self.column_end(column, hosts)?;
        let foot = Vec3::new(column.top.x, column.top.y, ROOT_TOP_MM);
        let diameter_mm = (column.radius * 2.0).clamp(MIN_ROD_DIAMETER_MM, MAX_BRACE_DIAMETER_MM);
        if along_shaft(shafts, foot, column.top, diameter_mm) {
            return None;
        }
        log::trace!("Post under support {support} from {foot} to {knot} ({} joints)", joints.len());
        Some(KickstandLayout {
            support,
            knot,
            joints,
            foot,
            root_diameter_mm: (diameter_mm * ROOT_DIAMETER_RATIO).clamp(MIN_ROOT_DIAMETER_MM, MAX_ROOT_DIAMETER_MM),
            diameter_mm,
        })
    }

    /// Whether the strut from `start` up to `end` runs through the shaft of another support the
    /// piece touches, with the piece covering the way to it: then it is a shorter strut from
    /// that support, and a brace runs on in line with it.
    fn runs_through(&self, piece: &Piece, hosts: &[HostLine], start: Vec3, end: BraceEnd, diameter_mm: f32) -> bool {
        let shortest = min_brace_mm(diameter_mm);
        let direction = (end.point - start).normalized();
        let mut others: Vec<usize> = piece.ends.iter().map(|e| e.support).filter(|&s| s != end.support).collect();
        others.sort_unstable();
        others.dedup();
        others.into_iter().any(|other| {
            hosts[other].line.windows(2).any(|w| {
                let at = closest_to_line(w[0], w[1], start, direction);
                at.z >= ROOT_TOP_MM + THROUGH_ROOT_MM
                    && distance_to_segment(at, start, end.point) < BRACE_THROUGH_MM
                    && (at - start).length() >= shortest
                    && (at - end.point).length() >= shortest
                    && self.brace_along(piece, start, at).is_some()
            })
        })
    }

    /// The kickstand a piece forms from `start` up to `top` on its support's shaft, standing on
    /// the plate below `start`, or on a column standing at `column` when given: a strut fitted
    /// through the piece's cross-sections, meeting the support's shaft where it passes closest.
    /// None when the piece does not cover the fitted strut, the strut runs along that shaft (the
    /// support's own lower segment) or along any traced shaft, or it is too short.
    #[allow(clippy::too_many_arguments)]
    fn fit_leg(
        &self,
        piece: &Piece,
        hosts: &[HostLine],
        top: BraceEnd,
        start: Vec3,
        column: Option<Vec3>,
        diameter_mm: f32,
        shafts: &[Shaft],
    ) -> Option<KickstandLayout> {
        let (origin, direction) = self
            .fit_line(&piece.blobs, start, top.point, diameter_mm / 2.0 + BRACE_MARGIN_MM)
            .unwrap_or((start, (top.point - start).normalized()));
        let direction = if direction.z < 0.0 { -direction } else { direction };
        if direction.z <= 0.0 {
            return None;
        }
        let along = |z: f32| origin + direction * ((z - origin.z) / direction.z);
        // The knot goes where the strut's line passes closest to the support's shaft, near
        // where the piece touches it.
        let line = &hosts[top.support].line;
        let (knot, near) = line
            .windows(2)
            .map(|w| (closest_to_line(w[0], w[1], origin, direction), w))
            .filter(|(at, _)| (*at - top.point).length() <= min_brace_mm(diameter_mm))
            .min_by(|(a, _), (b, _)| {
                let miss = |p: Vec3| (p - along(p.z)).length();
                miss(*a).total_cmp(&miss(*b))
            })?;
        // The strut leaves the root top, or the top of the column, where the strut's line
        // passes closest to the column.
        let (foot, joints) = match column {
            None => (along(ROOT_TOP_MM), Vec::new()),
            Some(base) => {
                let foot = Vec3::new(base.x, base.y, ROOT_TOP_MM);
                let joint = closest_to_line(foot, Vec3::new(base.x, base.y, start.z + 1.0), origin, direction);
                if joint.z < ROOT_TOP_MM + MIN_SHAFT_LENGTH_MM || along_shaft(shafts, foot, joint, diameter_mm) {
                    return None;
                }
                (foot, vec![joint])
            }
        };
        let bottom = joints.last().copied().unwrap_or(foot);
        let strut = along(knot.z) - along(bottom.z);
        let lean = (strut.x * strut.x + strut.y * strut.y).sqrt() / strut.z;
        if knot.z < bottom.z + MIN_SHAFT_LENGTH_MM
            || strut.length() < MIN_KICKSTAND_MM
            || !(MIN_KICKSTAND_LEAN..=MAX_KICKSTAND_LEAN).contains(&lean)
        {
            return None;
        }
        // The shaft the strut meets, the one below when the knot is at a joint: a strut carrying
        // on in line with it is that support's own lower segment.
        let radius = hosts[top.support].radius;
        let host = line
            .windows(2)
            .filter(|w| distance_to_segment(knot, w[0], w[1]) <= radius)
            .min_by(|a, b| a[0].z.total_cmp(&b[0].z))
            .unwrap_or(near);
        if (host[1] - host[0]).normalized().dot(strut.normalized()).abs() > BRACE_PARALLEL_COS
            || along_shaft(shafts, bottom, knot, diameter_mm)
            || self.brace_along(piece, along(bottom.z.max(start.z)), knot).is_none()
        {
            return None;
        }
        Some(KickstandLayout {
            support: top.support,
            knot,
            joints,
            foot,
            root_diameter_mm: (diameter_mm * ROOT_DIAMETER_RATIO).clamp(MIN_ROOT_DIAMETER_MM, MAX_ROOT_DIAMETER_MM),
            diameter_mm,
        })
    }

    /// The two braces of an X between the supports at ends `i` and `j` of a piece, when they
    /// meet each support in one junction, one above the other: each brace runs from the lower
    /// part of one junction to the upper part of the other (the lower and upper thirds of where
    /// the piece touches the support), with its diameter. The parts lie at least as far apart
    /// as the piece is thick (`diameter_mm`; a level bar's junction is as tall as the bar), the
    /// braces cross, one rising and one falling, and both run along the middles of the piece's
    /// cross-sections ([`X_CENTRE_MM`]) while the line between the junctions' middles does not:
    /// a single thick bar between supports close together also covers lines crossing it, but
    /// off its middle, and a level bar among others meeting the same junctions runs along it.
    fn crossing(&self, piece: &Piece, i: usize, j: usize, diameter_mm: f32) -> Option<[(Vec3, Vec3, f32); 2]> {
        let parts = |k: usize| {
            let touches = &piece.touches[k];
            let (lo, hi) = (touches[0].z, touches[touches.len() - 1].z);
            let third = (hi - lo) / 3.0;
            let mean = |keep: &dyn Fn(f32) -> bool| {
                let (sum, n) =
                    touches.iter().filter(|t| keep(t.z)).fold((Vec3::default(), 0), |(s, n), t| (s + *t, n + 1));
                sum * (1.0 / n as f32)
            };
            let (lower, upper) = (mean(&|z| z <= lo + third), mean(&|z| z >= hi - third));
            (upper.z - lower.z >= diameter_mm).then_some((lower, upper))
        };
        let ((a_low, a_high), (b_low, b_high)) = (parts(i)?, parts(j)?);
        let (a, b) = (piece.ends[i].point, piece.ends[j].point);
        if b_high.z <= a_low.z || a_high.z <= b_low.z || self.centred_along(piece, a, b).is_some() {
            return None;
        }
        let rising = self.centred_along(piece, a_low, b_high)?;
        let falling = self.centred_along(piece, a_high, b_low)?;
        Some([(a_low, b_high, rising), (a_high, b_low, falling)])
    }

    /// The diameter of the brace a piece forms from `p` to `q`, when the piece's
    /// cross-sections cover the line between them.
    fn brace_along(&self, piece: &Piece, p: Vec3, q: Vec3) -> Option<f32> {
        self.covered_along(piece, p, q, None)
    }

    /// [`Graph::brace_along`], with the line also running within [`X_CENTRE_MM`] of the middles
    /// of the cross-sections covering it.
    fn centred_along(&self, piece: &Piece, p: Vec3, q: Vec3) -> Option<f32> {
        self.covered_along(piece, p, q, Some(X_CENTRE_MM))
    }

    /// The diameter of the brace a piece forms from `p` to `q`, when the piece's
    /// cross-sections cover the line between them, with their middles within `centred` of it
    /// when given.
    fn covered_along(&self, piece: &Piece, p: Vec3, q: Vec3, centred: Option<f32>) -> Option<f32> {
        let tested = (q - p).length() * (1.0 - 2.0 * BRACE_END_SKIP);
        let samples = ((tested / BRACE_SAMPLE_MM).ceil() as usize).max(3);
        let allowed_misses = samples - (samples as f32 * BRACE_COVERAGE).ceil() as usize;
        let mut widths = Vec::with_capacity(samples);
        for k in 0..samples {
            let t = BRACE_END_SKIP + (1.0 - 2.0 * BRACE_END_SKIP) * (k as f32 + 0.5) / samples as f32;
            let s = p + (q - p) * t;
            let layer = ((s.z - self.floor) / self.step).round() as i64;
            let (x, y) = cell2([s.x, s.y], BLOB_CELL_MM);
            let cover = (layer - 1..=layer + 1)
                .filter_map(|l| u32::try_from(l).ok())
                .flat_map(|l| piece.cells.get(&(l, x, y)).into_iter().flatten())
                .map(|&b| &self.blobs[b as usize])
                .find(|blob| {
                    // The line at the blob's own height, for a blob in a neighbouring layer.
                    let at =
                        if (q.z - p.z).abs() > self.step { p + (q - p) * ((blob.z - p.z) / (q.z - p.z)) } else { s };
                    blob.rect.holds([s.x, s.y], BRACE_MARGIN_MM)
                        && centred.is_none_or(|reach| (blob.x - at.x).hypot(blob.y - at.y) <= reach)
                });
            match cover {
                Some(blob) => widths.push(blob.half_min * 2.0),
                None if k + 1 - widths.len() > allowed_misses => return None,
                None => {}
            }
        }
        widths.sort_by(f32::total_cmp);
        Some(widths[widths.len() / 2].clamp(MIN_ROD_DIAMETER_MM, MAX_BRACE_DIAMETER_MM))
    }

    /// Where the shaft through `nodes` (top down) continues at height `z`.
    fn predict(&self, nodes: &[u32], lean: Option<[f32; 2]>, z: f32) -> (f32, f32) {
        self.course(nodes, lean, z).unwrap_or_else(|| {
            let last = self.blobs[*nodes.last().expect("walk has a start") as usize];
            (last.x, last.y)
        })
    }

    /// Where the shaft through `nodes` (top down) is heading at height `z`: along its own
    /// direction once they give it, or else along `lean` (horizontal run per unit of descent)
    /// from the last of them.
    fn course(&self, nodes: &[u32], lean: Option<[f32; 2]>, z: f32) -> Option<(f32, f32)> {
        self.heading(nodes, z).or_else(|| {
            let last = self.blobs[*nodes.last().expect("walk has a start") as usize];
            lean.map(|[dx, dy]| (last.x + dx * (last.z - z), last.y + dy * (last.z - z)))
        })
    }

    /// Whether skipped blob `next`, a slice below the last of `run`, carries the run on: as wide
    /// as its first slice, and in line with its slices (within [`MAX_RUN_LEAN`] of upright when
    /// the run is one slice).
    fn carries_run(&self, run: &[u32], next: u32) -> bool {
        let b = &self.blobs[next as usize];
        let Some(&first) = run.first() else { return true };
        let f = &self.blobs[first as usize];
        if (b.half_min - f.half_min).abs() > f.half_min * (SHAFT_ALONE_RATIO - 1.0) + SHAFT_ALONE_SLACK_MM {
            return false;
        }
        let last = &self.blobs[*run.last().expect("non-empty") as usize];
        let steps = (run.len() - 1) as f32;
        let (ex, ey, tolerance) = if run.len() >= 2 {
            (last.x + (last.x - f.x) / steps, last.y + (last.y - f.y) / steps, SLOPE_TOLERANCE * self.step)
        } else {
            (last.x, last.y, MAX_RUN_LEAN * self.step)
        };
        (b.x - ex).hypot(b.y - ey) <= tolerance
    }

    /// [`Graph::predict`] when `nodes` are long enough to give the shaft's direction.
    fn heading(&self, nodes: &[u32], z: f32) -> Option<(f32, f32)> {
        let last = self.blobs[*nodes.last().expect("walk has a start") as usize];
        let a = nodes.iter().rev().map(|&i| self.blobs[i as usize]).find(|b| b.z - last.z >= DIRECTION_SPAN_MM)?;
        let t = (z - last.z) / (last.z - a.z);
        Some((last.x + (last.x - a.x) * t, last.y + (last.y - a.y) * t))
    }
}

/// One slice's cross-section as blobs. Each closed outline is its own blob, so separate
/// shells stay apart even where they overlap, and an outline lying wholly inside another (a
/// shaft inside its joint ball, or inside the raft) joins the one around it.
fn layer_blobs(triangles: &[Triangle], band: &[u32], shell_of: &[u32], z: f32) -> Vec<Section> {
    let (segments, shells): (Vec<[[f32; 2]; 2]>, Vec<u32>) = band
        .iter()
        .filter_map(|&i| cross_section(&triangles[i as usize], z).map(|segment| (segment, shell_of[i as usize])))
        .unzip();
    if segments.is_empty() {
        return Vec::new();
    }

    // Segments sharing an end point belong to one outline.
    let mut parent: Vec<u32> = (0..segments.len() as u32).collect();
    let mut ends: FxHashMap<(i32, i32), u32> = FxHashMap::default();
    for (i, segment) in segments.iter().enumerate() {
        for &p in segment {
            let (cx, cy) = cell2(p, OUTLINE_SNAP_MM);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(&j) = ends.get(&(cx + dx, cy + dy)) {
                        union(&mut parent, i as u32, j);
                    }
                }
            }
            ends.entry((cx, cy)).or_insert(i as u32);
        }
    }

    let mut outlines: Vec<Outline> = Vec::new();
    let mut slot: FxHashMap<u32, usize> = FxHashMap::default();
    for (i, segment) in segments.iter().enumerate() {
        let root = find(&mut parent, i as u32);
        let k = *slot.entry(root).or_insert_with(|| {
            outlines.push(Outline { rect: Rect::point(segment[0]), segments: Vec::new(), shell: shells[i] });
            outlines.len() - 1
        });
        outlines[k].rect.grow(segment[0]);
        outlines[k].rect.grow(segment[1]);
        outlines[k].segments.push(i as u32);
    }
    merge_enclosed(&segments, outlines)
}

/// One closed cross-section outline: its box and the indices of its segments.
struct Outline {
    rect: Rect,
    segments: Vec<u32>,
    shell: u32,
}

/// One blob of a slice: its box, its width across its widest direction, and the shell of its
/// largest outline.
struct Section {
    rect: Rect,
    width: f32,
    shell: u32,
}

/// Merges each outline lying wholly inside a larger one into it.
fn merge_enclosed(segments: &[[[f32; 2]; 2]], outlines: Vec<Outline>) -> Vec<Section> {
    let section = |members: &[&Outline]| {
        let mut rect = members[0].rect;
        for o in &members[1..] {
            rect.union(&o.rect);
        }
        let points = members.iter().flat_map(|o| o.segments.iter().flat_map(|&i| segments[i as usize]));
        let shell = members.iter().max_by(|a, b| a.rect.area().total_cmp(&b.rect.area())).map_or(NONE, |o| o.shell);
        Section { rect, width: cross_width(points), shell }
    };
    if outlines.len() < 2 {
        return outlines.iter().map(|o| section(&[o])).collect();
    }
    let mut grid: FxHashMap<(i32, i32), Vec<u32>> = FxHashMap::default();
    for (i, o) in outlines.iter().enumerate() {
        for c in o.rect.cells(BLOB_CELL_MM) {
            grid.entry(c).or_default().push(i as u32);
        }
    }
    let mut parent: Vec<u32> = (0..outlines.len() as u32).collect();
    for (i, inner) in outlines.iter().enumerate() {
        let centre = inner.rect.centre();
        for &j in grid.get(&cell2(centre, BLOB_CELL_MM)).into_iter().flatten() {
            let outer = &outlines[j as usize];
            if j as usize != i
                && outer.rect.area() > inner.rect.area()
                && outer.rect.encloses(&inner.rect)
                && encircles(segments, &outer.segments, centre)
            {
                union(&mut parent, i as u32, j);
            }
        }
    }
    let mut groups: Vec<Vec<&Outline>> = Vec::new();
    let mut slot: FxHashMap<u32, usize> = FxHashMap::default();
    for (i, o) in outlines.iter().enumerate() {
        let root = find(&mut parent, i as u32);
        let k = *slot.entry(root).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[k].push(o);
    }
    groups.iter().map(|g| section(g)).collect()
}

/// The width of `points` across their widest direction, from [`WIDTH_DIRECTIONS`]
/// directions. A horizontal slice of a tilted round shaft is an ellipse, and its width across
/// the long axis is the shaft's own diameter, where its box would read wide. For a shaft
/// modelled as a regular polygon it is the width across opposite corners, the size the shaft
/// was modelled at, where the narrowest width would be across flats.
fn cross_width(points: impl Iterator<Item = [f32; 2]> + Clone) -> f32 {
    let extents: Vec<f32> = (0..WIDTH_DIRECTIONS)
        .map(|k| {
            let (s, c) = (std::f32::consts::PI * k as f32 / WIDTH_DIRECTIONS as f32).sin_cos();
            let (lo, hi) = points
                .clone()
                .map(|[x, y]| x * c + y * s)
                .fold((f32::MAX, f32::MIN), |(lo, hi), t| (lo.min(t), hi.max(t)));
            hi - lo
        })
        .collect();
    let widest = (0..WIDTH_DIRECTIONS).max_by(|&a, &b| extents[a].total_cmp(&extents[b])).unwrap_or(0);
    extents[(widest + WIDTH_DIRECTIONS / 2) % WIDTH_DIRECTIONS]
}

/// Whether `p` lies inside the outline made of `outline`'s segments (even-odd rule).
fn encircles(segments: &[[[f32; 2]; 2]], outline: &[u32], p: [f32; 2]) -> bool {
    let mut inside = false;
    for &i in outline {
        let [a, b] = segments[i as usize];
        if (a[1] > p[1]) != (b[1] > p[1]) && a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) > p[0] {
            inside = !inside;
        }
    }
    inside
}

/// The segment where the plane at height `z` cuts the triangle.
fn cross_section(t: &Triangle, z: f32) -> Option<[[f32; 2]; 2]> {
    let mut out = [[0.0; 2]; 2];
    let mut n = 0;
    for (a, b) in [(t.v0, t.v1), (t.v1, t.v2), (t.v2, t.v0)] {
        if (a.z < z) != (b.z < z) {
            // Cut each edge from the same end, so the triangles sharing it get the same point.
            let (a, b) = if (a.z, a.x, a.y) <= (b.z, b.x, b.y) { (a, b) } else { (b, a) };
            let s = (z - a.z) / (b.z - a.z);
            if n < 2 {
                out[n] = [a.x + (b.x - a.x) * s, a.y + (b.y - a.y) * s];
            }
            n += 1;
        }
    }
    (n == 2).then_some(out)
}

fn find(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        parent[x as usize] = parent[parent[x as usize] as usize];
        x = parent[x as usize];
    }
    x
}

fn union(parent: &mut [u32], a: u32, b: u32) {
    let (a, b) = (find(parent, a), find(parent, b));
    if a != b {
        parent[b as usize] = a;
    }
}

#[derive(Clone, Copy)]
struct SurfacePoint {
    point: Vec3,
    normal: Vec3,
}

/// A support shell that is a single tapered rod, such as a tip cone.
#[derive(Debug, Clone, Copy)]
struct Cone {
    narrow: Vec3,
    narrow_radius: f32,
    wide: Vec3,
    wide_radius: f32,
}

/// The support mesh's closed shells: triangles joined by shared vertices.
struct Shells {
    /// Each triangle's shell.
    of: Vec<u32>,
    /// Each shell's cone, when it is one.
    cones: Vec<Option<Cone>>,
}

impl Shells {
    fn new(triangles: &[Triangle]) -> Self {
        let key = |v: Vec3| {
            let q = |c: f32| (c / SHELL_WELD_MM).round() as i32;
            (q(v.x), q(v.y), q(v.z))
        };
        let mut parent: Vec<u32> = (0..triangles.len() as u32).collect();
        let mut first: FxHashMap<(i32, i32, i32), u32> = FxHashMap::default();
        for (i, t) in triangles.iter().enumerate() {
            for v in [t.v0, t.v1, t.v2] {
                let other = *first.entry(key(v)).or_insert(i as u32);
                union(&mut parent, i as u32, other);
            }
        }
        let mut id: FxHashMap<u32, u32> = FxHashMap::default();
        let mut members: Vec<Vec<u32>> = Vec::new();
        let of: Vec<u32> = (0..triangles.len() as u32)
            .map(|i| {
                let root = find(&mut parent, i);
                let shell = *id.entry(root).or_insert_with(|| {
                    members.push(Vec::new());
                    members.len() as u32 - 1
                });
                members[shell as usize].push(i);
                shell
            })
            .collect();
        let cones = members
            .par_iter()
            .map(|m| if m.len() <= MAX_CONE_TRIANGLES { fit_cone(triangles, m) } else { None })
            .collect();
        Shells { of, cones }
    }

    fn cone(&self, shell: u32) -> Option<Cone> {
        self.cones.get(shell as usize).copied().flatten()
    }
}

/// The tip cone a shell is: a rod that tapers from its narrow end and may then run on at its
/// full width (a tip and a stretch of shaft modelled as one piece). Every vertex lies on that
/// wall, on an end cap (flat, or rounded at the wide end), or on a ball as wide as the rod around
/// where the taper ends (a tip and its joint modelled as one piece); the cone ends where the
/// taper reaches the full width.
fn fit_cone(triangles: &[Triangle], members: &[u32]) -> Option<Cone> {
    let verts: Vec<Vec3> = members
        .iter()
        .flat_map(|&i| {
            let t = &triangles[i as usize];
            [t.v0, t.v1, t.v2]
        })
        .collect();
    let mean = verts.iter().fold(Vec3::default(), |sum, v| sum + *v) * (1.0 / verts.len() as f32);
    // Principal direction by power iteration.
    let mut axis = Vec3::new(0.3, 0.2, 1.0).normalized();
    for _ in 0..30 {
        let next = verts.iter().fold(Vec3::default(), |sum, v| {
            let d = *v - mean;
            sum + d * d.dot(axis)
        });
        if next.length_sq() < 1e-12 {
            return None;
        }
        axis = next.normalized();
    }
    let along: Vec<(f32, f32)> = verts
        .iter()
        .map(|v| {
            let d = *v - mean;
            let t = d.dot(axis);
            (t, (d - axis * t).length())
        })
        .collect();
    let (lo, hi) = along.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
    let length = hi - lo;
    let band = length * CONE_END_BAND;
    let end_radius = |end: f32| along.iter().filter(|p| (p.0 - end).abs() <= band).map(|p| p.1).fold(0.0, f32::max);
    // The narrow end is the thinner one; distances are measured from it.
    let (narrow_t, narrow_radius, toward) =
        if end_radius(lo) <= end_radius(hi) { (lo, end_radius(lo), 1.0) } else { (hi, end_radius(hi), -1.0) };
    let wide_radius = along.iter().map(|p| p.1).fold(0.0, f32::max);
    if length < 2.0 * wide_radius || narrow_radius > wide_radius * CONE_TAPER {
        return None;
    }
    let tolerance = CONE_TOLERANCE_MM + wide_radius * CONE_TOLERANCE_RATIO;
    // The taper ends at the first vertex as wide as the rod.
    let taper = along
        .iter()
        .filter(|p| p.1 >= wide_radius - tolerance)
        .map(|p| (p.0 - narrow_t) * toward)
        .fold(f32::MAX, f32::min);
    if taper <= band {
        return None;
    }
    let wall = |s: f32| narrow_radius + (wide_radius - narrow_radius) * (s / taper).min(1.0);
    let on_rod = along.iter().all(|&(t, r)| {
        let s = (t - narrow_t) * toward;
        // The wide end may close in a rounded cap, no longer than the rod's radius.
        let at_cap = s <= band || s >= length - band || (s >= taper && s >= length - wide_radius - tolerance);
        let on_ball = ((s - taper).hypot(r) - wide_radius).abs() <= tolerance;
        on_ball || (r <= wall(s) + tolerance && (r >= wall(s) - tolerance || at_cap))
    });
    on_rod.then(|| Cone {
        narrow: mean + axis * narrow_t,
        narrow_radius,
        wide: mean + axis * (narrow_t + taper * toward),
        wide_radius,
    })
}

/// Nearest-point lookups on the model surface.
struct SurfaceIndex<'a> {
    triangles: &'a [Triangle],
    cells: FxHashMap<(i32, i32, i32), Vec<u32>>,
}

fn cell3(p: Vec3) -> (i32, i32, i32) {
    let c = |v: f32| (v / SURFACE_CELL_MM).floor() as i32;
    (c(p.x), c(p.y), c(p.z))
}

impl<'a> SurfaceIndex<'a> {
    fn new(triangles: &'a [Triangle]) -> Self {
        let mut cells: FxHashMap<(i32, i32, i32), Vec<u32>> = FxHashMap::default();
        for (i, t) in triangles.iter().enumerate() {
            let lo = cell3(Vec3::new(
                t.v0.x.min(t.v1.x).min(t.v2.x),
                t.v0.y.min(t.v1.y).min(t.v2.y),
                t.v0.z.min(t.v1.z).min(t.v2.z),
            ));
            let hi = cell3(Vec3::new(
                t.v0.x.max(t.v1.x).max(t.v2.x),
                t.v0.y.max(t.v1.y).max(t.v2.y),
                t.v0.z.max(t.v1.z).max(t.v2.z),
            ));
            for x in lo.0..=hi.0 {
                for y in lo.1..=hi.1 {
                    for z in lo.2..=hi.2 {
                        cells.entry((x, y, z)).or_default().push(i as u32);
                    }
                }
            }
        }
        Self { triangles, cells }
    }

    /// Where the shaft through `points` (top down) leaves the model, and the index of the first
    /// point outside it. A tip is usually buried a little way into the model, so its top is
    /// not where it touches. None when no point is outside (or the model's winding is
    /// inverted).
    fn entry(&self, points: &[Vec3]) -> Option<(SurfacePoint, usize)> {
        let mut previous: Option<(Vec3, f32)> = None;
        for (i, &p) in points.iter().enumerate() {
            let side = self.nearest(p, ENTRY_SEARCH_MM).map(|hit| (p - hit.point).dot(hit.normal));
            match (side, previous) {
                (Some(side), _) if side < 0.0 => previous = Some((p, side)),
                // Outside: the crossing lies between this point and the last inside one.
                (side, Some((inside, inside_side))) => {
                    let side = side.unwrap_or(ENTRY_SEARCH_MM);
                    let crossing = inside + (p - inside) * (inside_side / (inside_side - side));
                    return self.nearest(crossing, ENTRY_SEARCH_MM).map(|hit| (hit, i));
                }
                (Some(_), None) => return self.nearest(p, ENTRY_SEARCH_MM).map(|hit| (hit, i)),
                (None, None) => return None,
            }
        }
        None
    }

    /// The model surface an end of the support at `p` touches: within [`CONTACT_MAX_MM`], or
    /// within [`BURIED_CONTACT_MAX_MM`] when `p` is inside the model.
    fn contact(&self, p: Vec3) -> Option<SurfacePoint> {
        let hit = self.nearest(p, BURIED_CONTACT_MAX_MM)?;
        ((p - hit.point).length() <= CONTACT_MAX_MM || (p - hit.point).dot(hit.normal) < 0.0).then_some(hit)
    }

    /// The closest surface point within `max` of `p`, with its triangle's face normal.
    fn nearest(&self, p: Vec3, max: f32) -> Option<SurfacePoint> {
        let reach = (max / SURFACE_CELL_MM).ceil() as i32;
        let (cx, cy, cz) = cell3(p);
        let mut best: Option<(f32, Vec3, u32)> = None;
        // Ring by ring outward from p's own cell: nothing in ring `k` is nearer than `k - 1` cells.
        for k in 0..=reach {
            let nearest_in_ring = ((k - 1).max(0) as f32 * SURFACE_CELL_MM).powi(2);
            if best.is_some_and(|(d, _, _)| d <= nearest_in_ring) {
                break;
            }
            for x in -k..=k {
                for y in -k..=k {
                    for z in -k..=k {
                        if x.abs().max(y.abs()).max(z.abs()) != k {
                            continue;
                        }
                        for &i in self.cells.get(&(cx + x, cy + y, cz + z)).into_iter().flatten() {
                            let t = &self.triangles[i as usize];
                            let q = closest_on_triangle(p, t.v0, t.v1, t.v2);
                            let d = (q - p).length_sq();
                            if d <= max * max && best.is_none_or(|(b, _, j)| d < b || (d == b && i < j)) {
                                best = Some((d, q, i));
                            }
                        }
                    }
                }
            }
        }
        best.map(|(_, point, i)| {
            let t = &self.triangles[i as usize];
            SurfacePoint { point, normal: (t.v1 - t.v0).cross(t.v2 - t.v0).normalized() }
        })
    }
}

/// Closest point to `p` on triangle `abc` (Ericson, Real-Time Collision Detection 5.1.5).
pub(super) fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = va + vb + vc;
    if denom.abs() < f32::EPSILON {
        return a;
    }
    a + ab * (vb / denom) + ac * (vc / denom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::support_separation::tests::cuboid;

    /// Closed frustum from `a` (radius `ra`) to `b` (radius `rb`) with 16 sides.
    fn frustum(a: Vec3, b: Vec3, ra: f32, rb: f32) -> Vec<Triangle> {
        let axis = (b - a).normalized();
        let helper = if axis.z.abs() < 0.9 { Vec3::new(0.0, 0.0, 1.0) } else { Vec3::new(1.0, 0.0, 0.0) };
        let u = axis.cross(helper).normalized();
        let v = axis.cross(u);
        let ring = |c: Vec3, r: f32, k: usize| {
            let t = std::f32::consts::TAU * k as f32 / 16.0;
            c + u * (r * t.cos()) + v * (r * t.sin())
        };
        let mut out = Vec::new();
        for k in 0..16 {
            let (p0, p1, q0, q1) = (ring(a, ra, k), ring(a, ra, k + 1), ring(b, rb, k), ring(b, rb, k + 1));
            out.push(Triangle::new(p0, p1, q1, u));
            out.push(Triangle::new(p0, q1, q0, u));
            out.push(Triangle::new(a, p1, p0, -axis));
            out.push(Triangle::new(b, q0, q1, axis));
        }
        out
    }

    /// One closed upright tube through rings of the given radii, capped only at its ends.
    fn tube(rings: &[(Vec3, f32)]) -> Vec<Triangle> {
        let ring = |c: Vec3, r: f32, k: usize| {
            let t = std::f32::consts::TAU * k as f32 / 16.0;
            c + Vec3::new(r * t.cos(), r * t.sin(), 0.0)
        };
        let (first, last) = (rings[0], rings[rings.len() - 1]);
        let mut out = Vec::new();
        for k in 0..16 {
            for w in rings.windows(2) {
                let ((a, ra), (b, rb)) = (w[0], w[1]);
                let (p0, p1, q0, q1) = (ring(a, ra, k), ring(a, ra, k + 1), ring(b, rb, k), ring(b, rb, k + 1));
                out.push(Triangle::new(p0, p1, q1, Vec3::UP));
                out.push(Triangle::new(p0, q1, q0, Vec3::UP));
            }
            out.push(Triangle::new(first.0, ring(first.0, first.1, k + 1), ring(first.0, first.1, k), Vec3::UP));
            out.push(Triangle::new(last.0, ring(last.0, last.1, k), ring(last.0, last.1, k + 1), Vec3::UP));
        }
        out
    }

    /// A support in DragonFruit's anatomy: a tip cone buried 0.2 mm into the model at
    /// `contact`, a joint ball at the socket, then a shaft through `path` to the plate.
    fn support(contact: Vec3, path: &[Vec3]) -> Vec<Triangle> {
        let mut out = frustum(contact + Vec3::new(0.0, 0.0, 0.2), path[0], 0.12, 0.4);
        for w in path.windows(2) {
            out.extend(frustum(w[0], w[1], 0.4, 0.4));
            out.extend(frustum(w[0] + Vec3::new(0.0, 0.0, 0.5), w[0] - Vec3::new(0.0, 0.0, 0.5), 0.5, 0.5));
        }
        out
    }

    fn box_model(min: Vec3, max: Vec3) -> Vec<Triangle> {
        cuboid(min, max, 4)
    }

    fn close(a: Vec3, b: Vec3, within: f32) -> bool {
        (a - b).length() <= within
    }

    #[test]
    fn vertical_post_becomes_a_trunk_touching_the_model_underside() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let supports = support(Vec3::new(1.0, 2.0, 10.0), &[Vec3::new(1.0, 2.0, 8.0), Vec3::new(1.0, 2.0, 0.0)]);
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 1);
        let s = &layout.supports[0];
        assert!(close(s.tip.contact, Vec3::new(1.0, 2.0, 10.0), 0.1), "{}", s.tip.contact);
        assert!(close(s.tip.surface_normal, Vec3::new(0.0, 0.0, -1.0), 0.01), "{}", s.tip.surface_normal);
        assert!((7.4..=9.0).contains(&s.tip.socket.z), "{}", s.tip.socket);
        assert!((0.2..=0.4).contains(&s.tip.contact_diameter_mm), "{}", s.tip.contact_diameter_mm);
        assert!((0.7..=0.9).contains(&s.shaft_diameter_mm), "{}", s.shaft_diameter_mm);
        assert!(s.joints.is_empty(), "{:?}", s.joints);
        let SupportBase::Plate { x, y, .. } = s.base else { panic!("{:?}", s.base) };
        assert!((x - 1.0).abs() < 0.1 && (y - 2.0).abs() < 0.1, "{x} {y}");
    }

    #[test]
    fn bent_shaft_keeps_a_joint_at_the_bend() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let path = [Vec3::new(0.0, 0.0, 8.0), Vec3::new(3.0, 0.0, 5.0), Vec3::new(3.0, 0.0, 0.0)];
        let layout = reconstruct(&model, &support(Vec3::new(0.0, 0.0, 10.0), &path));

        assert_eq!(layout.supports.len(), 1);
        let s = &layout.supports[0];
        let on_path = |j: Vec3| path.windows(2).any(|w| distance_to_segment(j, w[0], w[1]) <= 0.4);
        assert!(s.joints.iter().all(|j| on_path(*j)), "{:?}", s.joints);
        assert!(s.joints.iter().any(|j| close(*j, path[1], 0.7)), "{:?}", s.joints);
        assert!(s.joints.windows(2).all(|w| w[0].z < w[1].z), "{:?}", s.joints);
        let SupportBase::Plate { x, .. } = s.base else { panic!("{:?}", s.base) };
        assert!((x - 3.0).abs() < 0.2, "{x}");
    }

    #[test]
    fn lower_tip_joining_another_shaft_is_a_branch_of_it() {
        let mut model = box_model(Vec3::new(-2.0, -2.0, 10.0), Vec3::new(2.0, 2.0, 15.0));
        model.extend(box_model(Vec3::new(2.0, -2.0, 9.0), Vec3::new(6.0, 2.0, 15.0)));
        let mut supports = support(Vec3::new(0.0, 0.0, 10.0), &[Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, 0.0)]);
        supports.extend(support(Vec3::new(4.0, 0.0, 9.0), &[Vec3::new(4.0, 0.0, 7.0), Vec3::new(0.3, 0.0, 4.0)]));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 2);
        assert!(matches!(layout.supports[0].base, SupportBase::Plate { .. }));
        assert!(close(layout.supports[1].tip.contact, Vec3::new(4.0, 0.0, 9.0), 0.1));
        let SupportBase::Shaft { parent, point } = layout.supports[1].base else {
            panic!("{:?}", layout.supports[1].base)
        };
        assert_eq!(parent, 0);
        assert!(point.x.abs() < 0.2 && (3.0..=5.0).contains(&point.z), "{point}");
        assert!(layout.braces.is_empty(), "{:?}", layout.braces);
    }

    #[test]
    fn tip_leaning_steeply_into_its_parent_keeps_its_cone() {
        // A trunk under one part of the model, and a tip on a lower part leaning 45 degrees
        // into the trunk's side, with no shaft of its own: a branch that is all tip.
        let mut model = box_model(Vec3::new(-2.0, -2.0, 10.0), Vec3::new(2.0, 2.0, 15.0));
        model.extend(box_model(Vec3::new(-6.0, -2.0, 9.0), Vec3::new(-2.0, 2.0, 15.0)));
        let mut supports = support(Vec3::new(0.0, 0.0, 10.0), &[Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, 0.0)]);
        supports.extend(frustum(Vec3::new(-3.2, 0.0, 9.2), Vec3::new(-0.3, 0.0, 6.3), 0.15, 0.35));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 2);
        let branch = &layout.supports[1];
        assert!(matches!(branch.base, SupportBase::Shaft { parent: 0, .. }), "{:?}", branch.base);
        let tip = &branch.tip;
        assert!(close(tip.contact, Vec3::new(-3.0, 0.0, 9.0), 0.2), "{}", tip.contact);
        // The socket lies on the tip's own line, most of the way to the trunk.
        assert!(distance_to_segment(tip.socket, Vec3::new(-3.0, 0.0, 9.0), Vec3::new(-0.3, 0.0, 6.3)) < 0.15);
        let length = (tip.socket - tip.contact).length();
        assert!(length >= 1.8, "{length}");
        assert!(tip.body_diameter_mm >= 0.5, "{}", tip.body_diameter_mm);
        assert!(branch.joints.is_empty(), "{:?}", branch.joints);
    }

    #[test]
    fn leaning_tip_socket_is_where_its_cone_ends() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        // A tip leaning 35 degrees from its contact, widening to the width of the upright shaft
        // it turns into 1.2 mm below the contact.
        let socket = Vec3::new(0.84, 0.0, 8.8);
        let mut supports = frustum(Vec3::new(-0.14, 0.0, 10.2), socket, 0.12, 0.33);
        supports.extend(frustum(socket, Vec3::new(0.84, 0.0, 0.0), 0.33, 0.33));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 1);
        let s = &layout.supports[0];
        assert!(close(s.tip.socket, socket, 0.3), "{}", s.tip.socket);
        assert!((0.55..=0.7).contains(&s.tip.body_diameter_mm), "{}", s.tip.body_diameter_mm);
        assert!(s.joints.is_empty(), "{:?}", s.joints);
    }

    #[test]
    fn buried_tip_contact_is_as_wide_as_the_tip_where_it_leaves_the_model() {
        // A tip buried 0.4 mm in the model: 0.2 mm across at its buried end and about 0.37 mm
        // where it leaves the model. Its taper changes on the way to the shaft, so it is no
        // cone shell of its own and is measured from its slices.
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let kink = Vec3::new(0.0, 0.0, 9.7);
        let mut supports = frustum(Vec3::new(0.0, 0.0, 10.4), kink, 0.1, 0.25);
        supports.extend(frustum(kink, Vec3::new(0.0, 0.0, 8.0), 0.25, 0.4));
        supports.extend(frustum(Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, 0.0), 0.4, 0.4));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 1);
        let tip = &layout.supports[0].tip;
        assert!(close(tip.contact, Vec3::new(0.0, 0.0, 10.0), 0.1), "{}", tip.contact);
        assert!((0.33..=0.45).contains(&tip.contact_diameter_mm), "{}", tip.contact_diameter_mm);
    }

    #[test]
    fn cone_tip_takes_its_socket_and_widths_from_the_cone() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        // A tip cone of its own, 0.3 mm at its buried narrow end and 0.8 mm at its wide end,
        // leaning 40 degrees into a joint ball on top of an upright shaft.
        let lean = Vec3::new(40f32.to_radians().sin(), 0.0, -40f32.to_radians().cos());
        let contact = Vec3::new(0.0, 0.0, 10.0);
        let socket = contact + lean * 2.0;
        let mut supports = frustum(contact - lean * 0.25, socket, 0.15, 0.4);
        supports.extend(frustum(socket + Vec3::new(0.0, 0.0, 0.5), socket - Vec3::new(0.0, 0.0, 0.5), 0.5, 0.5));
        supports.extend(frustum(socket - Vec3::new(0.0, 0.0, 0.05), Vec3::new(socket.x, 0.0, 0.0), 0.4, 0.4));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 1);
        let tip = &layout.supports[0].tip;
        assert!(close(tip.socket, socket, 0.05), "{}", tip.socket);
        // As wide as the cone where it leaves the model, a ninth of the way from its narrow end.
        let at_surface = 0.3 + 0.5 * 0.25 / 2.25;
        assert!((tip.contact_diameter_mm - at_surface).abs() < 0.02, "{}", tip.contact_diameter_mm);
        assert!((tip.body_diameter_mm - 0.8).abs() < 0.03, "{}", tip.body_diameter_mm);
        assert!(layout.supports[0].joints.is_empty(), "{:?}", layout.supports[0].joints);
    }

    #[test]
    fn cone_tip_closed_by_a_rounded_cap_ends_where_its_taper_does() {
        // A tip cone 3.7 mm long from 0.53 mm to 1.49 mm across, its wide end closed by a
        // hemisphere, as one shell.
        let (narrow, wide, radius) = (Vec3::new(1.0, 2.0, 10.0), Vec3::new(1.0, 2.0, 6.3), 0.744);
        let mut rings = vec![(narrow, 0.265), (wide, radius)];
        for degrees in [30.0f32, 55.0, 80.0] {
            let (sin, cos) = degrees.to_radians().sin_cos();
            rings.push((wide - Vec3::new(0.0, 0.0, radius * sin), radius * cos));
        }
        let piece = tube(&rings);
        let cone = fit_cone(&piece, &(0..piece.len() as u32).collect::<Vec<_>>()).expect("a cone");
        assert!(close(cone.narrow, narrow, 0.02) && close(cone.wide, wide, 0.05), "{cone:?}");
        assert!((cone.wide_radius - radius).abs() < 0.01, "{cone:?}");
    }

    #[test]
    fn cone_tip_running_into_its_joint_ball_ends_at_the_ball_centre() {
        // A tip and its joint ball as one shell, rising from a rounded narrow end (as in Death x
        // Tiles' strut feet): the wall swells onto the ball before reaching its full width, so
        // the cone is read up to the ball's centre.
        let at = |s: f32| Vec3::new(1.0, 2.0, 4.0 + s);
        let profile = [
            (0.011, 0.082),
            (0.045, 0.157),
            (0.097, 0.221),
            (0.165, 0.268),
            (0.963, 0.67),
            (1.332, 0.749),
            (1.693, 0.639),
            (1.955, 0.366),
            (2.04, 0.1),
        ];
        let piece = tube(&profile.map(|(s, r)| (at(s), r)));
        let cone = fit_cone(&piece, &(0..piece.len() as u32).collect::<Vec<_>>()).expect("a cone");
        assert!(close(cone.narrow, at(0.011), 0.02) && close(cone.wide, at(1.332), 0.05), "{cone:?}");
        // The narrow end is the widest ring within a tenth of the cone's length of its end: the
        // ring 0.165 mm up the rounded foot, not the foot's tip.
        assert!((cone.narrow_radius - 0.268).abs() < 0.01, "{cone:?}");
        assert!((cone.wide_radius - 0.749).abs() < 0.01, "{cone:?}");
    }

    /// A graph of round blobs `(x, layer, radius)` on 0.1 mm layers, linked layer to layer
    /// where their boxes overlap, as slicing links them.
    fn hand_graph(blobs: &[(f32, u32, f32)]) -> Graph {
        let blobs: Vec<(f32, f32, u32, f32)> = blobs.iter().map(|&(x, layer, r)| (x, 0.0, layer, r)).collect();
        hand_graph_xy(&blobs)
    }

    /// Round blobs at (x, y) in 0.1 mm layers, of radius r, linked where their boxes overlap.
    fn hand_graph_xy(blobs: &[(f32, f32, u32, f32)]) -> Graph {
        let step = 0.1;
        let blobs: Vec<Blob> = blobs
            .iter()
            .map(|&(x, y, layer, r)| Blob {
                x,
                y,
                z: layer as f32 * step,
                half_min: r,
                half_max: r,
                layer,
                rect: Rect { min: [x - r, y - r], max: [x + r, y + r] },
                shell: 0,
            })
            .collect();
        let (mut up, mut down) = (vec![Vec::new(); blobs.len()], vec![Vec::new(); blobs.len()]);
        for (i, a) in blobs.iter().enumerate() {
            for (j, b) in blobs.iter().enumerate() {
                if b.layer == a.layer + 1 && a.rect.overlaps(&b.rect, LINK_MARGIN_MM) {
                    up[i].push(j as u32);
                    down[j].push(i as u32);
                }
            }
        }
        let slopes = vec![Cell::new(f32::NAN); blobs.len()];
        Graph { blobs, up, down, floor: 0.0, step, slopes }
    }

    #[test]
    fn post_through_a_brace_junction_reads_upright() {
        // An upright post (x = 0) that a junction off its line (x = 0.5) swallows for four
        // layers, with a brace leaving the junction downward at 45 degrees. The slice nearest
        // each slice runs from the junction down the brace; the post carries on straight.
        let mut blobs: Vec<(f32, u32, f32)> =
            (0..=70).filter(|l| !(52..=55).contains(l)).map(|layer| (0.0, layer, 0.4)).collect();
        blobs.extend((52..=55).map(|layer| (0.5, layer, 0.9)));
        blobs.extend((30..=51).map(|layer| (0.6 + 0.1 * (51 - layer) as f32, layer, 0.3)));
        let graph = hand_graph(&blobs);
        let at = |x: f32, layer: u32| {
            graph.blobs.iter().position(|b| b.layer == layer && (b.x - x).abs() < 0.01).expect("a blob") as u32
        };

        let post = graph.descent_slope(at(0.0, 70));
        assert!(post < 0.05, "{post}");
        let brace = graph.descent_slope(at(0.9, 48));
        assert!(brace > 0.9, "{brace}");
    }

    #[test]
    fn walk_keeps_to_a_post_through_a_junction_off_its_line() {
        // An upright 1.42 mm post (x = 0) that a junction off its line swallows for four
        // layers, where it merges into one outline with a rung, as in a unioned strut and rung.
        let walk_post = |junction: (f32, f32), neighbour: Option<f32>| {
            let (jx, jr) = junction;
            let mut blobs: Vec<(f32, u32, f32)> =
                (0..=60).filter(|l| !(30..=33).contains(l)).map(|layer| (0.0, layer, 0.71)).collect();
            blobs.extend((30..=33).map(|layer| (jx, layer, jr)));
            blobs.extend(neighbour.into_iter().flat_map(|x| (0..=60).map(move |layer| (x, layer, 0.71))));
            let graph = hand_graph(&blobs);
            let top = graph.blobs.iter().position(|b| b.layer == 60 && b.x.abs() < 0.01).expect("the post's top");
            let walk = graph.walk(top as u32, &vec![NONE; graph.blobs.len()], true, |_| true);
            assert!(matches!(walk.end, WalkEnd::Plate));
            walk.nodes.iter().map(|&i| graph.blobs[i as usize].x).collect::<Vec<_>>()
        };
        // The junction's middle 0.3 mm off the post, about as close as the post's own slices
        // drift: it is not a point on the post's line.
        let xs = walk_post((0.3, 1.0), None);
        assert!(xs.iter().all(|x| x.abs() < 0.01), "{xs:?}");
        // A junction only a fifth wider than the post, its middle 0.2 mm off: still not a point on
        // the post's line, or a run of them would turn the post towards the rung.
        let xs = walk_post((0.2, 0.85), None);
        assert!(xs.iter().all(|x| x.abs() < 0.01), "{xs:?}");
        // A neighbouring post the junction reaches: the way on is the post the walk is on, which
        // comes out of the junction upright, not the neighbour.
        let xs = walk_post((0.45, 1.15), Some(2.0));
        assert!(xs.iter().all(|x| x.abs() < 0.01), "{xs:?}");
    }

    #[test]
    fn walk_keeps_to_its_own_leaning_shaft_beside_an_upright_post() {
        // A shaft leaning 0.3 mm per mm away from an upright post whose slices touch its first
        // ones: the post is the more upright way down, but its middle lies beside the shaft.
        let mut blobs: Vec<(f32, u32, f32)> =
            (0..=40).map(|layer| (-0.03 * (40 - layer) as f32, layer, 0.33)).collect();
        blobs.extend((0..=45).map(|layer| (0.45, layer, 0.33)));
        let graph = hand_graph(&blobs);
        let top = graph.blobs.iter().position(|b| b.layer == 40 && b.x.abs() < 0.01).expect("the shaft's top");
        let walk = graph.walk(top as u32, &vec![NONE; graph.blobs.len()], true, |_| true);

        assert!(matches!(walk.end, WalkEnd::Plate));
        let xs: Vec<f32> = walk.visited.iter().map(|&i| graph.blobs[i as usize].x).collect();
        assert!(xs.iter().all(|&x| x < 0.1), "{xs:?}");
    }

    #[test]
    fn shaft_below_a_steeply_leaning_tip_is_walked_from_where_the_tip_ends() {
        // A tip leaning 0.8 mm per mm from its contact for 1.5 mm, widening into an upright shaft.
        let mut blobs: Vec<(f32, u32, f32)> = (45..=60)
            .map(|layer| (0.08 * (60 - layer) as f32, layer, 0.1 + 0.25 * (60 - layer) as f32 / 15.0))
            .collect();
        blobs.extend((0..45).map(|layer| (1.2, layer, 0.35)));
        let graph = hand_graph(&blobs);
        let top = graph.blobs.iter().position(|b| b.layer == 60).expect("the tip's top");
        let walk = graph.walk(top as u32, &vec![NONE; graph.blobs.len()], true, |_| true);

        assert!(matches!(walk.end, WalkEnd::Plate));
        // Every slice of the shaft from just below the tip down is a shaft point (the first few,
        // carrying on from the tip's last slice, are kept with the tip's).
        let layers: Vec<u32> = walk.nodes.iter().map(|&i| graph.blobs[i as usize].layer).filter(|&l| l < 45).collect();
        assert!(layers.first().is_some_and(|&l| l >= 38), "{layers:?}");
        assert!(layers.windows(2).all(|w| w[0] == w[1] + 1), "{layers:?}");
    }

    #[test]
    fn walk_keeps_to_its_own_leaning_shaft_past_an_upright_post_it_converges_on() {
        // A shaft leaning 0.3 mm per mm (x falling as it goes down, at y = 0.3) that passes an
        // upright post (x = y = 0) close enough for the post's middle to lie within the shaft's
        // own slices' boxes for a while: the post is the more upright way down there, but off to
        // the side of where the shaft is heading.
        let mut blobs: Vec<(f32, f32, u32, f32)> =
            (0..=60).map(|layer| (1.2 - 0.03 * (60 - layer) as f32, 0.3, layer, 0.33)).collect();
        blobs.extend((0..=45).map(|layer| (0.0, 0.0, layer, 0.33)));
        let graph = hand_graph_xy(&blobs);
        let top = graph.blobs.iter().position(|b| b.layer == 60).expect("the shaft's top");
        let walk = graph.walk(top as u32, &vec![NONE; graph.blobs.len()], true, |_| true);

        assert!(matches!(walk.end, WalkEnd::Plate));
        let ys: Vec<f32> = walk.visited.iter().map(|&i| graph.blobs[i as usize].y).collect();
        assert!(ys.iter().all(|&y| y > 0.2), "{ys:?}");
    }

    #[test]
    fn shaft_direction_is_measured_over_a_millimetre() {
        // 1.5 mm of a shaft leaning 0.3 mm per mm, its last slice 0.03 mm off its line (a
        // slice's middle wanders that much). Measured over the last millimetre, the heading 1 mm
        // further down is within 0.075 mm of the line; over half a millimetre that slice would
        // turn it too far, and over two the shaft is not yet long enough to give a heading.
        let blobs: Vec<(f32, f32, u32, f32)> = (45..=60)
            .map(|layer| (0.03 * (60 - layer) as f32, if layer == 45 { 0.03 } else { 0.0 }, layer, 0.3))
            .collect();
        let graph = hand_graph_xy(&blobs);
        let nodes: Vec<u32> = (0..graph.blobs.len() as u32).rev().collect();
        let (x, y) = graph.heading(&nodes, 3.5).expect("1.5 mm of shaft gives its heading");
        assert!((x - 0.75).hypot(y) < 0.075, "{x} {y}");
    }

    #[test]
    fn walk_keeps_to_its_own_gently_leaning_shaft_past_an_upright_post() {
        // A shaft leaning 0.1 mm per mm (x falling as it goes down, at y = 0.3) over an upright
        // post (x = y = 0) whose slices it overlaps all the way down. The post is the more upright
        // way at each fork, but the shaft's own way is nearly as upright and lies where the shaft
        // is heading, so the walk stays on it.
        let mut blobs: Vec<(f32, f32, u32, f32)> =
            (0..=60).map(|layer| (0.4 - 0.01 * (60 - layer) as f32, 0.3, layer, 0.33)).collect();
        blobs.extend((0..=50).map(|layer| (0.0, 0.0, layer, 0.33)));
        let graph = hand_graph_xy(&blobs);
        let top = graph.blobs.iter().position(|b| b.layer == 60).expect("the shaft's top");
        let walk = graph.walk(top as u32, &vec![NONE; graph.blobs.len()], true, |_| true);

        assert!(matches!(walk.end, WalkEnd::Plate));
        let ys: Vec<f32> = walk.visited.iter().map(|&i| graph.blobs[i as usize].y).collect();
        assert!(ys.iter().all(|&y| y > 0.2), "{ys:?}");
    }

    #[test]
    fn shaft_skipped_past_a_bend_is_walked_from_where_its_straight_run_began() {
        // A tip leaning 1.5 mm per mm from its contact for 1 mm, a joint much wider than the
        // shaft, then the shaft leaning 0.3 mm per mm back the other way. The walk skips the tip's
        // slices and the shaft's first ones until it gives up skipping; by then those already run
        // straight at one width, so the shaft is taken up from where they began.
        let mut blobs: Vec<(f32, u32, f32)> =
            (50..=60).map(|layer| (0.15 * (60 - layer) as f32, layer, 0.1 + 0.02 * (60 - layer) as f32)).collect();
        blobs.push((1.6, 49, 0.8));
        blobs.extend((0..=48).map(|layer| (1.6 - 0.03 * (48 - layer) as f32, layer, 0.33)));
        let graph = hand_graph(&blobs);
        let top = graph.blobs.iter().position(|b| b.layer == 60).expect("the tip's top");
        let walk = graph.walk(top as u32, &vec![NONE; graph.blobs.len()], true, |_| true);

        assert!(matches!(walk.end, WalkEnd::Plate));
        let layers: Vec<u32> = walk.nodes.iter().map(|&i| graph.blobs[i as usize].layer).filter(|&l| l < 49).collect();
        assert!(layers.first().is_some_and(|&l| l >= 47), "{layers:?}");
        assert!(layers.windows(2).all(|w| w[0] == w[1] + 1), "{layers:?}");
    }

    #[test]
    fn tip_cone_beside_another_contact_running_on_its_own_into_its_shaft_is_a_branch() {
        // Two tip cones meeting the model 0.4 mm apart: the first runs straight down into its
        // post, the second leans away from it for 3 mm and then runs into the post lower down.
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let at = |x: f32, z: f32| Vec3::new(x, 0.0, z);
        let mut supports = frustum(at(0.0, 10.4), at(0.0, 7.0), 0.12, 0.4);
        supports.extend(frustum(at(0.0, 7.0), at(0.0, 0.0), 0.4, 0.4));
        supports.extend(support(at(0.4, 10.0), &[at(1.5, 7.0), at(0.0, 5.0)]));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 2, "{:?}", layout.supports);
        let fan = layout.supports.iter().find(|s| close(s.tip.contact, at(0.4, 10.0), 0.2)).expect("the second cone");
        assert!(matches!(fan.base, SupportBase::Shaft { .. }), "{:?}", fan.base);
    }

    #[test]
    fn shaft_line_passes_by_a_junction_off_its_line() {
        // A 2 mm post that a junction off its line widens for 0.3 mm, as where it merges with a
        // brace: the junction's middle is no point on the post's line.
        let at = |x: f32, z: f32| Vec3::new(x, 0.0, z);
        let mut supports = frustum(at(0.0, 10.2), at(0.0, 8.0), 0.15, 1.0);
        supports.extend(tube(&[
            (at(0.0, 0.0), 1.0),
            (at(0.0, 5.0), 1.0),
            (at(0.35, 5.1), 1.2),
            (at(0.35, 5.4), 1.2),
            (at(0.0, 5.5), 1.0),
            (at(0.0, 8.05), 1.0),
        ]));
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 1);
        let s = &layout.supports[0];
        assert!(s.joints.is_empty(), "{:?}", s.joints);
    }

    #[test]
    fn support_ending_in_a_ball_buried_in_the_model_touches_where_it_enters() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        // A post whose top end is buried 1.9 mm into the model's underside.
        let mut supports = frustum(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 10.0), 0.75, 0.75);
        supports.extend(frustum(Vec3::new(0.0, 0.0, 9.9), Vec3::new(0.0, 0.0, 11.9), 0.8, 0.4));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 1);
        let s = &layout.supports[0];
        assert!(close(s.tip.contact, Vec3::new(0.0, 0.0, 10.0), 0.2), "{}", s.tip.contact);
        assert!(s.tip.socket.z < 10.0, "{}", s.tip.socket);
        let SupportBase::Plate { x, y, .. } = s.base else { panic!("{:?}", s.base) };
        assert!(x.abs() < 0.1 && y.abs() < 0.1, "{x} {y}");
    }

    #[test]
    fn nearest_surface_point_matches_a_search_of_every_triangle() {
        let model = cuboid(Vec3::new(-3.0, -2.0, 1.0), Vec3::new(4.0, 3.5, 6.0), 7);
        let surface = SurfaceIndex::new(&model);
        for p in [
            Vec3::new(0.3, 0.2, 3.1),
            Vec3::new(-2.6, -1.7, 1.4),
            Vec3::new(4.9, 0.0, 3.0),
            Vec3::new(1.1, 3.9, 6.8),
            Vec3::new(0.0, 0.0, 8.7),
        ] {
            let brute =
                model.iter().map(|t| (closest_on_triangle(p, t.v0, t.v1, t.v2) - p).length()).fold(f32::MAX, f32::min);
            let found = surface.nearest(p, 2.5).map(|hit| (hit.point - p).length());
            if brute <= 2.5 {
                assert!(found.is_some_and(|d| (d - brute).abs() < 1e-5), "{p}: {found:?} vs {brute}");
            } else {
                assert!(found.is_none(), "{p}: {found:?}");
            }
        }
    }

    #[test]
    fn support_that_is_one_cone_into_its_root_keeps_a_shaft() {
        // A short support modelled as a single cone from the model down to its root's top, with
        // the root below.
        let model = box_model(Vec3::new(-5.0, -5.0, 6.2), Vec3::new(5.0, 5.0, 10.0));
        let mut supports = frustum(Vec3::new(0.0, 0.0, 6.4), Vec3::new(0.0, 0.0, 2.1), 0.12, 0.5);
        supports.extend(frustum(Vec3::new(0.0, 0.0, 2.05), Vec3::new(0.0, 0.0, 0.0), 0.5, 1.5));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 1);
        let s = &layout.supports[0];
        assert!(matches!(s.base, SupportBase::Plate { .. }), "{:?}", s.base);
        assert!(s.tip.socket.z >= ROOT_TOP_MM + MIN_SHAFT_LENGTH_MM, "{}", s.tip.socket);
    }

    #[test]
    fn cone_fit_reads_a_taper_then_a_shaft_and_rejects_rods_and_balls() {
        let all = |t: &[Triangle]| fit_cone(t, &(0..t.len() as u32).collect::<Vec<_>>());
        // A tip and a stretch of shaft modelled as one piece: the cone ends where the taper does.
        let (narrow, wide) = (Vec3::new(1.0, 2.0, 10.0), Vec3::new(1.0, 2.0, 8.0));
        let piece = tube(&[(narrow, 0.15), (wide, 0.5), (Vec3::new(1.0, 2.0, 5.0), 0.5)]);
        let cone = all(&piece).expect("a cone");
        assert!(close(cone.narrow, narrow, 0.02) && close(cone.wide, wide, 0.05), "{cone:?}");
        assert!((cone.narrow_radius - 0.15).abs() < 0.01 && (cone.wide_radius - 0.5).abs() < 0.01, "{cone:?}");

        assert!(all(&frustum(narrow, wide, 0.4, 0.4)).is_none());
        assert!(all(&frustum(narrow, narrow - Vec3::new(0.0, 0.0, 0.8), 0.5, 0.5)).is_none());
    }

    #[test]
    fn rod_barely_tapering_is_not_a_tip_cone() {
        // A rod narrowing from 0.9 to 0.8 mm along its 3 mm: a shaft, not a tip.
        let rod = frustum(Vec3::new(1.0, 2.0, 10.0), Vec3::new(1.0, 2.0, 7.0), 0.4, 0.45);
        assert!(fit_cone(&rod, &(0..rod.len() as u32).collect::<Vec<_>>()).is_none());
    }

    #[test]
    fn cone_fit_allows_a_wall_a_little_off_true() {
        let all = |t: &[Triangle]| fit_cone(t, &(0..t.len() as u32).collect::<Vec<_>>());
        let at = |z: f32| Vec3::new(1.0, 2.0, z);
        // A thin tip cone (0.2 to 0.6 mm over 2 mm) whose wall bulges 0.018 mm halfway along.
        let thin = tube(&[(at(10.0), 0.1), (at(9.0), 0.218), (at(8.0), 0.3), (at(7.0), 0.3)]);
        let cone = all(&thin).expect("a cone");
        assert!(close(cone.wide, at(8.0), 0.05), "{cone:?}");
        // A thick one (0.6 to 2 mm over 4 mm) whose wall bulges 0.04 mm: a share of its width.
        let thick = tube(&[(at(10.0), 0.3), (at(8.0), 0.69), (at(6.0), 1.0), (at(4.0), 1.0)]);
        let cone = all(&thick).expect("a cone");
        assert!(close(cone.wide, at(6.0), 0.1), "{cone:?}");
    }

    #[test]
    fn raft_under_the_supports_does_not_end_the_trunk() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut supports = support(Vec3::new(1.0, 1.0, 10.0), &[Vec3::new(1.0, 1.0, 8.0), Vec3::new(1.0, 1.0, 0.5)]);
        supports.extend(cuboid(Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 1.0), 4));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 1);
        let SupportBase::Plate { x, y, .. } = layout.supports[0].base else { panic!() };
        assert!((x - 1.0).abs() < 0.1 && (y - 1.0).abs() < 0.1, "{x} {y}");
        assert!(layout.braces.is_empty(), "{:?}", layout.braces);
        assert!(layout.kickstands.is_empty(), "{:?}", layout.kickstands);
    }

    #[test]
    fn diagonal_brace_does_not_pull_the_trunk_off_its_post() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut supports = support(Vec3::new(0.0, 0.0, 10.0), &[Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, 0.0)]);
        supports.extend(support(Vec3::new(3.0, 0.0, 10.0), &[Vec3::new(3.0, 0.0, 8.0), Vec3::new(3.0, 0.0, 0.0)]));
        // Brace from high on the first post down to the second.
        supports.extend(frustum(Vec3::new(0.0, 0.0, 6.0), Vec3::new(3.0, 0.0, 3.0), 0.3, 0.3));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 2);
        for s in &layout.supports {
            let SupportBase::Plate { x, .. } = s.base else { panic!("{:?}", s.base) };
            assert!((x - s.tip.contact.x).abs() < 0.2, "{x} under {}", s.tip.contact);
            assert!(s.joints.is_empty(), "{:?}", s.joints);
        }
    }

    /// Two posts 3 mm apart under the model, at x = 0 and x = 3, with `extra` between them.
    fn two_posts(extra: &[Triangle]) -> SupportLayout {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut supports = support(Vec3::new(0.0, 0.0, 10.0), &[Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, 0.0)]);
        supports.extend(support(Vec3::new(3.0, 0.0, 10.0), &[Vec3::new(3.0, 0.0, 8.0), Vec3::new(3.0, 0.0, 0.0)]));
        supports.extend_from_slice(extra);
        reconstruct(&model, &supports)
    }

    /// The post (0 or 1) whose shaft is at `x`.
    fn post_at(layout: &SupportLayout, x: f32) -> usize {
        layout.supports.iter().position(|s| (s.tip.contact.x - x).abs() < 0.2).expect("a post there")
    }

    #[test]
    fn diagonal_brace_between_posts_is_found_with_its_ends_on_each_post() {
        let layout = two_posts(&frustum(Vec3::new(0.0, 0.0, 6.0), Vec3::new(3.0, 0.0, 3.0), 0.3, 0.3));

        assert_eq!(layout.braces.len(), 1, "{:?}", layout.braces);
        let brace = &layout.braces[0];
        assert_eq!(brace.lower.support, post_at(&layout, 3.0));
        assert_eq!(brace.upper.support, post_at(&layout, 0.0));
        assert!(close(brace.lower.point, Vec3::new(3.0, 0.0, 3.0), 0.6), "{}", brace.lower.point);
        assert!(close(brace.upper.point, Vec3::new(0.0, 0.0, 6.0), 0.6), "{}", brace.upper.point);
        assert!((0.5..=0.75).contains(&brace.diameter_mm), "{}", brace.diameter_mm);
    }

    #[test]
    fn web_one_slice_thick_between_posts_is_not_a_brace() {
        // A level web 0.12 mm thick between the posts: it shows in a slice or two at most.
        let layout = two_posts(&cuboid(Vec3::new(0.0, -0.1, 5.02), Vec3::new(3.0, 0.1, 5.14), 2));
        assert!(layout.braces.is_empty(), "{:?}", layout.braces);
    }

    #[test]
    fn crossing_braces_are_two_braces() {
        let mut x = frustum(Vec3::new(0.0, 0.0, 6.0), Vec3::new(3.0, 0.0, 3.0), 0.3, 0.3);
        x.extend(frustum(Vec3::new(0.0, 0.0, 3.0), Vec3::new(3.0, 0.0, 6.0), 0.3, 0.3));
        let layout = two_posts(&x);

        assert_eq!(layout.braces.len(), 2, "{:?}", layout.braces);
        for brace in &layout.braces {
            assert_ne!(brace.lower.support, brace.upper.support);
            assert!(brace.upper.point.z - brace.lower.point.z > 2.0, "{brace:?}");
        }
    }

    #[test]
    fn crossing_braces_meeting_each_post_in_one_junction_are_two_braces() {
        // An X whose braces meet each post 1.4 mm apart, so where the piece touches a post is
        // one junction; a single diagonal of the same size beside it is one brace.
        let mut x = frustum(Vec3::new(0.0, 0.0, 6.2), Vec3::new(3.0, 0.0, 4.8), 0.35, 0.35);
        x.extend(frustum(Vec3::new(0.0, 0.0, 4.8), Vec3::new(3.0, 0.0, 6.2), 0.35, 0.35));
        let layout = two_posts(&x);
        assert_eq!(layout.braces.len(), 2, "{:?}", layout.braces);
        for brace in &layout.braces {
            assert_ne!(brace.lower.support, brace.upper.support);
            assert!(brace.upper.point.z - brace.lower.point.z > 0.8, "{brace:?}");
        }

        let layout = two_posts(&frustum(Vec3::new(0.0, 0.0, 4.8), Vec3::new(3.0, 0.0, 6.2), 0.35, 0.35));
        assert_eq!(layout.braces.len(), 1, "{:?}", layout.braces);
        let brace = &layout.braces[0];
        assert!(close(brace.lower.point, Vec3::new(0.0, 0.0, 4.8), 0.4), "{brace:?}");
        assert!(close(brace.upper.point, Vec3::new(3.0, 0.0, 6.2), 0.4), "{brace:?}");
    }

    #[test]
    fn shortest_brace_scales_with_its_diameter() {
        // Thin lattice braces are short; thick ones keep a minimum near the old fixed 2 mm.
        assert_eq!(min_brace_mm(0.6), MIN_BRACE_MIN_MM);
        assert!((min_brace_mm(1.2) - 1.8).abs() < 1e-5);
        assert!((min_brace_mm(2.0) - 3.0).abs() < 1e-5);

        // Two thin posts 1.7 mm apart with a thin level bar between them: a brace, though
        // shorter than a thick brace could be.
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut supports = Vec::new();
        for x in [0.0, 1.7] {
            supports.extend(frustum(Vec3::new(x, 0.0, 10.2), Vec3::new(x, 0.0, 8.0), 0.1, 0.3));
            supports.extend(frustum(Vec3::new(x, 0.0, 8.0), Vec3::new(x, 0.0, 0.0), 0.3, 0.3));
        }
        supports.extend(frustum(Vec3::new(0.0, 0.0, 5.0), Vec3::new(1.7, 0.0, 5.0), 0.25, 0.25));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 2);
        assert_eq!(layout.braces.len(), 1, "{:?}", layout.braces);
        let brace = &layout.braces[0];
        assert_ne!(brace.lower.support, brace.upper.support);
        assert!(brace.diameter_mm < 1.0, "{}", brace.diameter_mm);
    }

    #[test]
    fn brace_bowed_off_its_line_by_less_than_its_width_is_a_brace() {
        // A diagonal brace between the posts whose middle bows 0.5 mm sideways, so its slices
        // stand up to that far off the straight line between its ends: still one brace.
        let (a, m, b) = (Vec3::new(0.0, 0.0, 6.0), Vec3::new(1.5, 0.5, 4.5), Vec3::new(3.0, 0.0, 3.0));
        let mut bowed = frustum(a, m, 0.3, 0.3);
        bowed.extend(frustum(m, b, 0.3, 0.3));
        let layout = two_posts(&bowed);

        assert_eq!(layout.braces.len(), 1, "{:?}", layout.braces);
        let brace = &layout.braces[0];
        assert_ne!(brace.lower.support, brace.upper.support);
    }

    #[test]
    fn brace_line_is_refitted_from_its_slices_when_a_quarter_of_it_shows() {
        // A 45 degree brace from (0, 6) down to (3, 3) of which only the middle 30% shows between
        // the junctions hiding its ends (as on unioned zigzag webs and hollow-model struts): the
        // slices still give its line. A single slice (a level bar's one long slice) does not.
        let slices: Vec<(f32, f32, u32, f32)> =
            (41..=49).map(|layer| (6.0 - layer as f32 * 0.1, 0.0, layer, 0.3)).collect();
        let graph = hand_graph_xy(&slices);
        let piece: Vec<u32> = (0..graph.blobs.len() as u32).collect();
        let (p, q) = (Vec3::new(0.0, 0.0, 6.0), Vec3::new(3.0, 0.0, 3.0));
        let (mean, direction) = graph.fit_line(&piece, p, q, 0.5).expect("a line through the slices");
        assert!(distance_to_segment(mean, p, q) < 0.01, "{mean}");
        assert!(direction.dot((q - p).normalized()).abs() > 0.999, "{direction}");

        let one = hand_graph_xy(&[(1.5, 0.0, 45, 1.5)]);
        assert!(one.fit_line(&[0], p, q, 0.5).is_none());
    }

    #[test]
    fn level_bar_between_posts_is_a_brace() {
        let layout = two_posts(&frustum(Vec3::new(0.0, 0.0, 5.0), Vec3::new(3.0, 0.0, 5.0), 0.3, 0.3));

        assert_eq!(layout.braces.len(), 1, "{:?}", layout.braces);
        let brace = &layout.braces[0];
        assert_ne!(brace.lower.support, brace.upper.support);
        for end in [brace.lower, brace.upper] {
            assert!((end.point.z - 5.0).abs() < 0.4, "{end:?}");
        }
    }

    #[test]
    fn brace_ending_on_a_tip_has_no_shaft_to_hang_from() {
        // A post at x = 0, and one at x = 3 whose tip cone runs 4 mm down from its contact; a
        // brace from the first post's shaft up to the second post's tip cone.
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut supports = support(Vec3::new(0.0, 0.0, 10.0), &[Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, 0.0)]);
        supports.extend(support(Vec3::new(3.0, 0.0, 10.0), &[Vec3::new(3.0, 0.0, 6.0), Vec3::new(3.0, 0.0, 0.0)]));
        supports.extend(frustum(Vec3::new(0.0, 0.0, 4.5), Vec3::new(3.0, 0.0, 8.5), 0.3, 0.3));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 2);
        let tipped = post_at(&layout, 3.0);
        assert!(layout.supports[tipped].tip.socket.z < 7.0, "{}", layout.supports[tipped].tip.socket);
        assert!(layout.braces.is_empty(), "{:?}", layout.braces);
    }

    #[test]
    fn posts_with_nothing_between_them_have_no_braces() {
        assert!(two_posts(&[]).braces.is_empty());
    }

    /// The model over a trunk at the origin, the trunk's support geometry, and `extra`.
    fn trunk_with(extra: &[Triangle]) -> SupportLayout {
        let model = box_model(Vec3::new(-6.0, -6.0, 12.0), Vec3::new(6.0, 6.0, 16.0));
        let mut supports = support(Vec3::new(0.0, 0.0, 12.0), &[Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, 0.0)]);
        supports.extend_from_slice(extra);
        reconstruct(&model, &supports)
    }

    #[test]
    fn legs_splayed_from_a_trunk_to_the_plate_are_kickstands() {
        // Two legs leaving the trunk 6 mm up, leaning 30 degrees out to the plate.
        let mut legs = frustum(Vec3::new(0.0, 0.0, 6.0), Vec3::new(3.5, 0.0, 0.0), 0.3, 0.3);
        legs.extend(frustum(Vec3::new(0.0, 0.0, 6.0), Vec3::new(0.0, -3.5, 0.0), 0.3, 0.3));
        let layout = trunk_with(&legs);

        assert_eq!(layout.supports.len(), 1);
        let SupportBase::Plate { x, y, .. } = layout.supports[0].base else { panic!("{:?}", layout.supports[0].base) };
        assert!(x.abs() < 0.1 && y.abs() < 0.1, "{x} {y}");
        assert!(layout.braces.is_empty(), "{:?}", layout.braces);
        assert_eq!(layout.kickstands.len(), 2, "{:?}", layout.kickstands);
        for out in [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, -1.0, 0.0)] {
            let leg = layout.kickstands.iter().find(|k| k.foot.dot(out) > 1.0).expect("a leg that way");
            assert_eq!(leg.support, 0);
            assert!(leg.joints.is_empty(), "{:?}", leg.joints);
            assert!(close(leg.knot, Vec3::new(0.0, 0.0, 6.0), 0.3), "{}", leg.knot);
            // Where the leg passes the root top: 4 mm down from the knot, 7/3 mm out.
            let expected = out * (3.5 / 3.0 * 2.0) + Vec3::new(0.0, 0.0, ROOT_TOP_MM);
            assert!(close(leg.foot, expected, 0.2), "{} for {}", leg.foot, expected);
            assert!((0.55..=0.75).contains(&leg.diameter_mm), "{}", leg.diameter_mm);
            assert!((MIN_ROOT_DIAMETER_MM..=MAX_ROOT_DIAMETER_MM).contains(&leg.root_diameter_mm));
        }
    }

    #[test]
    fn post_with_a_strut_leaning_off_its_top_is_a_kickstand_with_a_column() {
        // A post rising from the plate 4 mm from the trunk, and a strut from its top to the trunk.
        let mut post = frustum(Vec3::new(4.0, 0.0, 0.0), Vec3::new(4.0, 0.0, 5.0), 0.3, 0.3);
        post.extend(frustum(Vec3::new(4.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 9.0), 0.3, 0.3));
        let layout = trunk_with(&post);

        assert_eq!(layout.supports.len(), 1);
        assert_eq!(layout.kickstands.len(), 1, "{:?}", layout.kickstands);
        let kickstand = &layout.kickstands[0];
        assert!(close(kickstand.foot, Vec3::new(4.0, 0.0, ROOT_TOP_MM), 0.1), "{}", kickstand.foot);
        assert_eq!(kickstand.joints.len(), 1);
        assert!(close(kickstand.joints[0], Vec3::new(4.0, 0.0, 5.0), 0.3), "{:?}", kickstand.joints);
        assert!(close(kickstand.knot, Vec3::new(0.0, 0.0, 9.0), 0.3), "{}", kickstand.knot);
    }

    #[test]
    fn strut_leaving_a_post_below_its_top_is_a_kickstand_with_a_column() {
        // A post rising 9 mm from the plate 4 mm from the trunk, and a strut leaving it 5 mm up
        // for the trunk; the post runs on above the strut.
        let mut post = frustum(Vec3::new(4.0, 0.0, 0.0), Vec3::new(4.0, 0.0, 9.0), 0.3, 0.3);
        post.extend(frustum(Vec3::new(4.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 9.0), 0.3, 0.3));
        let layout = trunk_with(&post);

        assert_eq!(layout.supports.len(), 1);
        assert_eq!(layout.kickstands.len(), 1, "{:?}", layout.kickstands);
        let kickstand = &layout.kickstands[0];
        assert!(close(kickstand.foot, Vec3::new(4.0, 0.0, ROOT_TOP_MM), 0.1), "{}", kickstand.foot);
        assert_eq!(kickstand.joints.len(), 1);
        assert!(close(kickstand.joints[0], Vec3::new(4.0, 0.0, 5.0), 0.3), "{:?}", kickstand.joints);
        assert!(close(kickstand.knot, Vec3::new(0.0, 0.0, 9.0), 0.3), "{}", kickstand.knot);
    }

    #[test]
    fn brace_to_a_post_hangs_from_the_kickstand_the_post_holds_up() {
        // A post with a strut leaning off its top onto the trunk, and a brace from the post 4 mm
        // up to the trunk 5.5 mm up.
        let mut post = frustum(Vec3::new(4.0, 0.0, 0.0), Vec3::new(4.0, 0.0, 7.0), 0.3, 0.3);
        post.extend(frustum(Vec3::new(4.0, 0.0, 7.0), Vec3::new(0.0, 0.0, 9.5), 0.3, 0.3));
        post.extend(frustum(Vec3::new(4.0, 0.0, 4.0), Vec3::new(0.0, 0.0, 5.5), 0.3, 0.3));
        let layout = trunk_with(&post);

        assert_eq!(layout.supports.len(), 1);
        assert_eq!(layout.kickstands.len(), 1, "{:?}", layout.kickstands);
        assert!(close(layout.kickstands[0].knot, Vec3::new(0.0, 0.0, 9.5), 0.3), "{:?}", layout.kickstands);
        assert_eq!(layout.braces.len(), 1, "{:?}", layout.braces);
        let brace = &layout.braces[0];
        assert_eq!((brace.lower.support, brace.lower.kickstand), (0, Some(0)), "{brace:?}");
        assert_eq!((brace.upper.support, brace.upper.kickstand), (0, None), "{brace:?}");
        assert!(close(brace.lower.point, Vec3::new(4.0, 0.0, 4.0), 0.6), "{brace:?}");
        assert!(close(brace.upper.point, Vec3::new(0.0, 0.0, 5.5), 0.6), "{brace:?}");
    }

    #[test]
    fn legs_splayed_from_a_post_to_the_plate_hang_from_it_as_a_post_of_its_own() {
        // A post 4 mm from the trunk rising 7 mm, a strut leaning off it 6 mm up onto the trunk,
        // and two legs splaying from it there down to the plate.
        let mut post = frustum(Vec3::new(4.0, 0.0, 0.0), Vec3::new(4.0, 0.0, 7.0), 0.3, 0.3);
        post.extend(frustum(Vec3::new(4.0, 0.0, 6.0), Vec3::new(0.0, 0.0, 10.0), 0.3, 0.3));
        let legs = [Vec3::new(7.5, 0.0, 0.0), Vec3::new(4.0, -4.0, 0.0)];
        for &foot in &legs {
            post.extend(frustum(foot, Vec3::new(4.0, 0.0, 6.0), 0.3, 0.3));
        }
        let layout = trunk_with(&post);

        assert_eq!(layout.supports.len(), 1);
        assert_eq!(layout.posts.len(), 1, "{:?}", layout.posts);
        let p = &layout.posts[0];
        assert!(close(p.foot, Vec3::new(4.0, 0.0, ROOT_TOP_MM), 0.1), "{}", p.foot);
        assert!(p.joints.is_empty(), "{:?}", p.joints);
        assert!(close(p.top, Vec3::new(4.0, 0.0, 7.0), 0.2), "{}", p.top);
        assert!((0.55..=0.75).contains(&p.diameter_mm), "{}", p.diameter_mm);
        assert!((MIN_ROOT_DIAMETER_MM..=MAX_ROOT_DIAMETER_MM).contains(&p.root_diameter_mm));

        // The legs are kickstands holding up the post, numbered on from the supports.
        assert_eq!(layout.kickstands.len(), 2, "{:?}", layout.kickstands);
        for foot in legs {
            let out = Vec3::new(foot.x - 4.0, foot.y, 0.0).normalized();
            let leg = layout.kickstands.iter().find(|k| (k.foot - p.foot).dot(out) > 1.0).expect("a leg that way");
            assert_eq!(leg.support, 1);
            assert!(leg.joints.is_empty(), "{:?}", leg.joints);
            assert!(close(leg.knot, Vec3::new(4.0, 0.0, 6.0), 0.4), "{}", leg.knot);
        }

        // The strut leaning off the post onto the trunk is a brace between them.
        assert_eq!(layout.braces.len(), 1, "{:?}", layout.braces);
        let brace = &layout.braces[0];
        assert_eq!((brace.lower.support, brace.lower.kickstand), (1, None), "{brace:?}");
        assert_eq!((brace.upper.support, brace.upper.kickstand), (0, None), "{brace:?}");
        assert!(close(brace.lower.point, Vec3::new(4.0, 0.0, 6.0), 0.6), "{brace:?}");
        assert!(close(brace.upper.point, Vec3::new(0.0, 0.0, 10.0), 0.6), "{brace:?}");

        // Without legs the post is a kickstand under the trunk, as DragonFruit builds its own.
        let mut lone = frustum(Vec3::new(4.0, 0.0, 0.0), Vec3::new(4.0, 0.0, 7.0), 0.3, 0.3);
        lone.extend(frustum(Vec3::new(4.0, 0.0, 6.0), Vec3::new(0.0, 0.0, 10.0), 0.3, 0.3));
        let layout = trunk_with(&lone);
        assert!(layout.posts.is_empty(), "{:?}", layout.posts);
        assert_eq!(layout.kickstands.len(), 1, "{:?}", layout.kickstands);
    }

    #[test]
    fn free_standing_post_braced_to_a_trunk_is_a_post_the_brace_hangs_from() {
        // A post 3 mm from the trunk rising 9 mm with nothing on its top, and a brace falling
        // from it 7 mm up to the trunk 4 mm up: it holds nothing up, so it is no kickstand.
        let mut post = frustum(Vec3::new(3.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 9.0), 0.3, 0.3);
        post.extend(frustum(Vec3::new(3.0, 0.0, 7.0), Vec3::new(0.0, 0.0, 4.0), 0.3, 0.3));
        let layout = trunk_with(&post);

        assert_eq!(layout.supports.len(), 1);
        assert!(layout.kickstands.is_empty(), "{:?}", layout.kickstands);
        assert_eq!(layout.posts.len(), 1, "{:?}", layout.posts);
        let p = &layout.posts[0];
        assert!(close(p.foot, Vec3::new(3.0, 0.0, ROOT_TOP_MM), 0.1), "{}", p.foot);
        assert!(close(p.top, Vec3::new(3.0, 0.0, 9.0), 0.2), "{}", p.top);
        assert_eq!(layout.braces.len(), 1, "{:?}", layout.braces);
        let brace = &layout.braces[0];
        assert_eq!((brace.lower.support, brace.upper.support), (0, 1), "{brace:?}");
        assert!(close(brace.lower.point, Vec3::new(0.0, 0.0, 4.0), 0.6), "{brace:?}");
        assert!(close(brace.upper.point, Vec3::new(3.0, 0.0, 7.0), 0.6), "{brace:?}");

        // A lone post, braced to nothing, is left out.
        let lone = trunk_with(&frustum(Vec3::new(3.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 9.0), 0.3, 0.3));
        assert!(lone.posts.is_empty(), "{:?}", lone.posts);
    }

    #[test]
    fn column_carries_on_past_a_junction_a_traced_walk_passed_through() {
        // An untraced post (x = 0) whose slices from 4 to 4.4 mm are a junction with a traced
        // support beside it, wider and off its line, which that support's walk passed.
        let blobs: Vec<(f32, u32, f32)> = (0..=80)
            .map(|layer| if (40..=44).contains(&layer) { (0.4, layer, 0.9) } else { (0.0, layer, 0.3) })
            .collect();
        let graph = hand_graph(&blobs);
        let mut seen = vec![false; graph.blobs.len()];
        let mut owner = vec![NONE; graph.blobs.len()];
        for i in 40..=44 {
            seen[i] = true;
            owner[i] = 0;
        }

        let column = graph.column(&seen, &owner, 0).expect("a column");
        assert!((column.top.z - 8.0).abs() < 0.05, "{}", column.top);
        assert!(column.into.is_none(), "{:?}", column.into.map(|i| i.1));
        assert!(column.junctions.iter().any(|j| (j.z - 4.2).abs() < 0.3), "{:?}", column.junctions);
    }

    #[test]
    fn column_ends_in_a_traced_support_it_merges_with_for_longer_than_a_junction() {
        // A post like that one, merged with the traced support beside it from 5 to 8 mm: taller
        // than any brace junction, so the post ends there, running into that support.
        let blobs: Vec<(f32, u32, f32)> = (0..=100)
            .map(|layer| if (50..=80).contains(&layer) { (0.4, layer, 0.9) } else { (0.0, layer, 0.3) })
            .collect();
        let graph = hand_graph(&blobs);
        let mut seen = vec![false; graph.blobs.len()];
        let mut owner = vec![NONE; graph.blobs.len()];
        for i in 50..=80 {
            seen[i] = true;
            owner[i] = 0;
        }

        let column = graph.column(&seen, &owner, 0).expect("a column");
        assert!(column.top.z < 5.0, "{}", column.top);
        assert!(column.into.is_some_and(|(support, _)| support == 0));
    }

    #[test]
    fn post_running_up_into_a_traced_shaft_is_a_kickstand_under_it() {
        // An untraced post (x = 0) whose slices run up into a slice of a traced shaft leaning
        // across above it (support 0, from (-2, 0, 4) to (2, 0, 8)).
        let mut blobs: Vec<(f32, u32, f32)> = (0..=60).map(|layer| (0.0, layer, 0.3)).collect();
        blobs.push((0.0, 61, 0.6));
        let graph = hand_graph(&blobs);
        let mut seen = vec![false; graph.blobs.len()];
        let mut owner = vec![NONE; graph.blobs.len()];
        seen[61] = true;
        owner[61] = 0;
        let host = SupportPath {
            tip: ContactTip {
                contact: Vec3::new(2.0, 0.0, 9.0),
                surface_normal: Vec3::new(0.0, 0.0, -1.0),
                from_outside: true,
                socket: Vec3::new(2.0, 0.0, 8.0),
                contact_diameter_mm: 0.3,
                body_diameter_mm: 0.6,
            },
            base: SupportBase::Plate { x: -2.0, y: 0.0, diameter_mm: 2.0 },
            joints: vec![Vec3::new(-2.0, 0.0, 4.0)],
            shaft_diameter_mm: 0.6,
        };
        let hosts = host_lines(std::slice::from_ref(&host), &[]);
        let (shafts, _) = traced_shafts(&hosts);

        let column = graph.column(&seen, &owner, 0).expect("a column");
        assert!((column.top.z - 6.0).abs() < 0.05, "{}", column.top);
        let kickstand = graph.post_under(&column, &hosts, &shafts).expect("a kickstand");
        assert_eq!(kickstand.support, 0);
        assert!(close(kickstand.foot, Vec3::new(0.0, 0.0, ROOT_TOP_MM), 0.05), "{}", kickstand.foot);
        assert!(kickstand.joints.is_empty(), "{:?}", kickstand.joints);
        assert!(close(kickstand.knot, Vec3::new(0.0, 0.0, 6.1), 0.1), "{}", kickstand.knot);
    }

    #[test]
    fn legs_running_into_the_raft_lean_alike_from_above_the_root_top() {
        // Two legs leaving a hub 3 mm up, leaning 0.7 mm per mm either way; below the root top
        // one runs into a stretch of raft whose middle lies 5 mm away.
        let mut blobs: Vec<(f32, u32, f32)> = vec![(0.0, 30, 0.6)];
        for layer in 0..30 {
            let out = 0.5 + 0.07 * (29 - layer) as f32;
            blobs.push((-out, layer, 0.3));
            blobs.push(if layer >= 20 { (out, layer, 0.3) } else { (5.0, layer, 4.0) });
        }
        let graph = hand_graph(&blobs);
        let at = |x: f32, layer: u32| {
            graph.blobs.iter().position(|b| b.layer == layer && (b.x - x).abs() < 0.01).expect("a blob") as u32
        };
        let (left, right) = (graph.descent_slope(at(-0.5, 29)), graph.descent_slope(at(0.5, 29)));
        assert!((left - right).abs() < SLOPE_TOLERANCE, "{left} {right}");

        // Below the root top a way is only worse when it stops short of the plate.
        let mut blobs: Vec<(f32, u32, f32)> = (0..=14).map(|layer| (0.0, layer, 0.3)).collect();
        blobs.extend((10..=14).map(|layer| (1.0, layer, 0.2)));
        let graph = hand_graph(&blobs);
        let at = |x: f32, layer: u32| {
            graph.blobs.iter().position(|b| b.layer == layer && (b.x - x).abs() < 0.01).expect("a blob") as u32
        };
        let (plate, stub) = (graph.descent_slope(at(0.0, 14)), graph.descent_slope(at(1.0, 14)));
        assert!(plate < stub, "{plate} {stub}");
    }

    #[test]
    fn braces_rafts_root_cones_and_bent_trunks_are_not_kickstands() {
        let brace = two_posts(&frustum(Vec3::new(0.0, 0.0, 6.0), Vec3::new(3.0, 0.0, 3.0), 0.3, 0.3));
        assert_eq!(brace.braces.len(), 1);
        assert!(brace.kickstands.is_empty(), "{:?}", brace.kickstands);

        // A brace from just above one post's root up to the other post joins two supports.
        let low = two_posts(&frustum(Vec3::new(3.0, 0.0, 2.6), Vec3::new(0.0, 0.0, 6.0), 0.3, 0.3));
        assert!(low.kickstands.is_empty(), "{:?}", low.kickstands);

        let mut raft = cuboid(Vec3::new(-10.0, -10.0, -1.0), Vec3::new(10.0, 10.0, 0.5), 4);
        // A root cone around the trunk's foot, the raft under it.
        raft.extend(frustum(Vec3::new(0.0, 0.0, -0.5), Vec3::new(0.0, 0.0, 2.0), 1.5, 0.45));
        let rooted = trunk_with(&raft);
        assert_eq!(rooted.supports.len(), 1);
        assert!(rooted.kickstands.is_empty(), "{:?}", rooted.kickstands);

        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let path = [Vec3::new(0.0, 0.0, 8.0), Vec3::new(3.0, 0.0, 5.0), Vec3::new(3.0, 0.0, 0.0)];
        let bent = reconstruct(&model, &support(Vec3::new(0.0, 0.0, 10.0), &path));
        assert!(bent.kickstands.is_empty(), "{:?}", bent.kickstands);

        // A second, untraced post standing beside the trunk.
        let post = trunk_with(&frustum(Vec3::new(0.65, 0.0, 0.0), Vec3::new(0.65, 0.0, 7.0), 0.3, 0.3));
        assert!(post.kickstands.is_empty(), "{:?}", post.kickstands);
    }

    #[test]
    fn leaning_shaft_reads_its_own_diameter() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        // An 0.8 mm shaft leaning 50 degrees along the diagonal: its slices are ellipses turned
        // 45 degrees, whose box reads about 1.05 mm wide.
        let lean = 8.0 * 50f32.to_radians().tan() / 2f32.sqrt();
        let supports = support(Vec3::new(0.0, 0.0, 10.0), &[Vec3::new(0.0, 0.0, 8.0), Vec3::new(lean, lean, 0.0)]);
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 1);
        let shaft = layout.supports[0].shaft_diameter_mm;
        assert!((0.75..=0.85).contains(&shaft), "{shaft}");
    }

    #[test]
    fn cross_width_is_the_ellipse_minor_axis_and_a_polygon_corner_width() {
        // A slice of a 0.5 mm shaft leaning 60 degrees, turned 30 degrees in the plane.
        let (s, c) = 30f32.to_radians().sin_cos();
        let ellipse = (0..64).map(|k| {
            let t = std::f32::consts::TAU * k as f32 / 64.0;
            let (x, y) = (0.25 / 60f32.to_radians().cos() * t.cos(), 0.25 * t.sin());
            [x * c - y * s, x * s + y * c]
        });
        assert!((cross_width(ellipse) - 0.5).abs() < 0.01);
        // A 16-sided shaft of 0.65 mm across corners reads 0.65, not 0.638 across flats.
        let polygon = (0..16).map(|k| {
            let t = std::f32::consts::TAU * k as f32 / 16.0;
            [0.325 * t.cos(), 0.325 * t.sin()]
        });
        assert!((cross_width(polygon) - 0.65).abs() < 0.002);
    }

    #[test]
    fn overlapping_separate_posts_stay_separate_at_their_own_width() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        // Two 0.8 mm posts whose shells overlap by 0.1 mm all the way down.
        let mut supports = support(Vec3::new(0.0, 0.0, 10.0), &[Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, 0.0)]);
        supports.extend(support(Vec3::new(0.7, 0.0, 10.0), &[Vec3::new(0.7, 0.0, 8.0), Vec3::new(0.7, 0.0, 0.0)]));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 2);
        for s in &layout.supports {
            let SupportBase::Plate { x, .. } = s.base else { panic!("{:?}", s.base) };
            assert!((x - s.tip.contact.x).abs() < 0.1, "{x} under {}", s.tip.contact);
            assert!((0.7..=0.9).contains(&s.shaft_diameter_mm), "{}", s.shaft_diameter_mm);
        }
    }

    #[test]
    fn strut_between_two_model_parts_keeps_both_contacts() {
        let mut model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        model.extend(box_model(Vec3::new(1.0, -3.0, 0.0), Vec3::new(6.0, 3.0, 6.0)));
        // A 0.5 mm rod from the upper part's underside down to the lower part's top, buried
        // a little way into each.
        let strut = frustum(Vec3::new(-0.05, 0.0, 10.2), Vec3::new(2.05, 0.0, 5.8), 0.25, 0.25);
        let layout = reconstruct(&model, &strut);

        assert_eq!(layout.supports.len(), 1);
        let s = &layout.supports[0];
        let SupportBase::Model(lower) = &s.base else { panic!("{:?}", s.base) };
        assert!(close(s.tip.contact, Vec3::new(0.0, 0.0, 10.0), 0.3), "{}", s.tip.contact);
        assert!(close(lower.contact, Vec3::new(1.95, 0.0, 6.0), 0.3), "{}", lower.contact);
        assert!(close(lower.surface_normal, Vec3::new(0.0, 0.0, 1.0), 0.01), "{}", lower.surface_normal);
        assert!(s.tip.socket.z > lower.socket.z);
        assert!((0.4..=0.65).contains(&s.shaft_diameter_mm), "{}", s.shaft_diameter_mm);
    }

    /// The underside of a ceiling over z = 20 and the top of a floor under z = 4, as in a
    /// hollowed model's cavity.
    fn hollow() -> Vec<Triangle> {
        let mut model = box_model(Vec3::new(-6.0, -4.0, 20.0), Vec3::new(8.0, 4.0, 24.0));
        model.extend(box_model(Vec3::new(-6.0, -4.0, 0.0), Vec3::new(8.0, 4.0, 4.0)));
        model
    }

    /// A 1.5 mm strut from the ceiling to the floor at `x`: a tip cone down from the ceiling, a
    /// shaft, and a tip cone up from the floor, each a shell of its own.
    fn hollow_strut(x: f32) -> Vec<Triangle> {
        let mut out = frustum(Vec3::new(x, 0.0, 20.2), Vec3::new(x, 0.0, 16.0), 0.27, 0.75);
        out.extend(frustum(Vec3::new(x, 0.0, 16.05), Vec3::new(x, 0.0, 5.0), 0.75, 0.75));
        out.extend(frustum(Vec3::new(x, 0.0, 3.8), Vec3::new(x, 0.0, 5.05), 0.22, 0.75));
        out
    }

    #[test]
    fn strut_whose_foot_leans_off_beside_an_upright_strut_keeps_its_own_foot() {
        // Two struts side by side, their 1.5 mm shafts overlapping; one runs straight down to the
        // floor, the other bends 50 degrees into a foot leaning away from it.
        let mut supports = hollow_strut(1.0);
        supports.extend(frustum(Vec3::new(0.0, 0.0, 20.3), Vec3::new(0.0, 0.0, 16.0), 0.27, 0.75));
        supports.extend(frustum(Vec3::new(0.0, 0.0, 16.05), Vec3::new(0.0, 0.0, 7.0), 0.75, 0.75));
        supports.extend(frustum(Vec3::new(0.0, 0.0, 7.05), Vec3::new(-3.8, 0.0, 3.8), 0.75, 0.22));
        let layout = reconstruct(&hollow(), &supports);

        assert_eq!(layout.supports.len(), 2, "{:?}", layout.supports);
        let feet: Vec<Vec3> = layout
            .supports
            .iter()
            .map(|s| match &s.base {
                SupportBase::Model(lower) => lower.contact,
                other => panic!("{other:?}"),
            })
            .collect();
        assert!(feet.iter().any(|f| close(*f, Vec3::new(-3.4, 0.0, 4.0), 0.6)), "{feet:?}");
        assert!(feet.iter().any(|f| close(*f, Vec3::new(1.0, 0.0, 4.0), 0.3)), "{feet:?}");
    }

    #[test]
    fn struts_tied_by_zigzag_rungs_come_out_whole_with_their_rungs() {
        // Two struts 2.4 mm apart inside a hollow, tied by rungs as thick as they are, each
        // running down at 45 degrees from one strut to the other.
        let mut supports = hollow_strut(0.0);
        supports.extend(hollow_strut(2.4));
        let rungs = [(0.0, 15.0), (2.4, 12.6), (0.0, 10.2), (2.4, 7.8)];
        for w in rungs.windows(2) {
            let ((x0, z0), (x1, z1)) = (w[0], w[1]);
            supports.extend(frustum(Vec3::new(x0, 0.0, z0), Vec3::new(x1, 0.0, z1), 0.75, 0.75));
        }
        let layout = reconstruct(&hollow(), &supports);

        assert_eq!(layout.supports.len(), 2);
        for s in &layout.supports {
            let SupportBase::Model(lower) = &s.base else { panic!("{:?}", s.base) };
            assert!(
                close(lower.contact, Vec3::new(s.tip.contact.x, 0.0, 4.0), 0.2),
                "{} under {}",
                lower.contact,
                s.tip.contact
            );
            assert!(s.joints.is_empty(), "{:?}", s.joints);
        }
        assert_eq!(layout.braces.len(), 3, "{:?}", layout.braces);
        for brace in &layout.braces {
            assert_ne!(brace.lower.support, brace.upper.support);
            assert!(brace.upper.point.z - brace.lower.point.z > 1.0, "{brace:?}");
        }
    }

    /// The struts' contacts, upper and lower, panicking on any other support.
    fn strut_contacts(layout: &SupportLayout) -> Vec<(Vec3, Vec3)> {
        let contacts = layout.supports.iter().map(|s| match &s.base {
            SupportBase::Model(lower) => (s.tip.contact, lower.contact),
            other => panic!("{other:?}"),
        });
        contacts.collect()
    }

    #[test]
    fn struts_meeting_at_one_ceiling_contact_are_two_struts() {
        // Two struts whose tip cones start 0.3 mm apart on the ceiling and lean apart to shafts
        // 2.3 mm apart, each down to a foot of its own.
        let mut supports = Vec::new();
        for (top, x) in [(0.0, -1.0), (0.3, 1.3)] {
            supports.extend(frustum(Vec3::new(top, 0.0, 20.2), Vec3::new(x, 0.0, 16.0), 0.27, 0.75));
            supports.extend(frustum(Vec3::new(x, 0.0, 16.05), Vec3::new(x, 0.0, 5.0), 0.75, 0.75));
            supports.extend(frustum(Vec3::new(x, 0.0, 3.8), Vec3::new(x, 0.0, 5.05), 0.22, 0.75));
        }
        let layout = reconstruct(&hollow(), &supports);

        let struts = strut_contacts(&layout);
        assert_eq!(struts.len(), 2, "{struts:?}");
        for (top, x) in [(0.0, -1.0), (0.3, 1.3)] {
            assert!(
                struts.iter().any(|(upper, lower)| {
                    close(*upper, Vec3::new(top, 0.0, 20.0), 0.3) && close(*lower, Vec3::new(x, 0.0, 4.0), 0.2)
                }),
                "{struts:?}"
            );
        }
    }

    #[test]
    fn second_top_of_one_tip_is_not_a_support_of_its_own() {
        // A plate support whose tip has a second, thinner top 0.3 mm beside its own, merging into
        // the tip 0.5 mm below the model.
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut supports = support(Vec3::new(0.0, 0.0, 10.0), &[Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, 0.0)]);
        supports.extend(frustum(Vec3::new(0.3, 0.0, 10.1), Vec3::new(0.1, 0.0, 9.5), 0.1, 0.15));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 1, "{:?}", layout.supports);
    }

    #[test]
    fn strut_beside_another_keeps_its_own_foot_where_their_feet_merge() {
        // Two struts 0.85 mm apart on a diagonal, their 1.5 mm shafts overlapping: one runs
        // straight down to the floor, the other ends 3 mm above the floor in a joint ball, from
        // which its own foot cone leans 26 degrees away to the floor.
        let mut supports = hollow_strut(0.0);
        let (top, ball) = (Vec3::new(-0.9, -0.9, 20.1), Vec3::new(-0.6, -0.6, 7.0));
        supports.extend(frustum(top, Vec3::new(-0.6, -0.6, 16.0), 0.27, 0.75));
        supports.extend(frustum(Vec3::new(-0.6, -0.6, 16.05), ball, 0.75, 0.75));
        supports.extend(frustum(ball + Vec3::new(0.0, 0.0, 0.75), ball - Vec3::new(0.0, 0.0, 0.75), 0.75, 0.75));
        supports.extend(frustum(Vec3::new(-2.2, -0.6, 3.75), ball, 0.22, 0.7));
        let layout = reconstruct(&hollow(), &supports);

        let struts = strut_contacts(&layout);
        assert_eq!(struts.len(), 2, "{struts:?}");
        assert!(struts.iter().any(|(_, lower)| close(*lower, Vec3::new(0.0, 0.0, 4.0), 0.2)), "{struts:?}");
        assert!(struts.iter().any(|(_, lower)| close(*lower, Vec3::new(-2.08, -0.6, 4.0), 0.3)), "{struts:?}");
    }

    #[test]
    fn strut_tips_that_are_cones_take_their_sockets_and_widths_from_the_cones() {
        // A strut whose lower tip is a cone of its own, 0.3 mm at its buried narrow end and
        // 0.8 mm at its wide end, leaning 40 degrees up from the floor into a joint ball at the
        // foot of an upright shaft; its upper tip is a cone of its own too.
        let lean = Vec3::new(40f32.to_radians().sin(), 0.0, 40f32.to_radians().cos());
        let contact = Vec3::new(0.0, 0.0, 4.0);
        let socket = contact + lean * 2.0;
        let upper_socket = Vec3::new(socket.x, 0.0, 16.0);
        let mut supports = frustum(contact - lean * 0.25, socket, 0.15, 0.4);
        supports.extend(frustum(socket - Vec3::new(0.0, 0.0, 0.5), socket + Vec3::new(0.0, 0.0, 0.5), 0.5, 0.5));
        supports.extend(frustum(
            socket + Vec3::new(0.0, 0.0, 0.05),
            upper_socket + Vec3::new(0.0, 0.0, 0.05),
            0.4,
            0.4,
        ));
        supports.extend(frustum(Vec3::new(socket.x, 0.0, 20.2), upper_socket, 0.15, 0.4));
        let layout = reconstruct(&hollow(), &supports);

        assert_eq!(layout.supports.len(), 1);
        let s = &layout.supports[0];
        let SupportBase::Model(lower) = &s.base else { panic!("{:?}", s.base) };
        assert!(close(lower.contact, contact, 0.1), "{}", lower.contact);
        assert!(close(lower.socket, socket, 0.05), "{}", lower.socket);
        // Each contact is as wide as its cone where it leaves the model.
        let lower_at_surface = 0.3 + 0.5 * 0.25 / 2.25;
        assert!((lower.contact_diameter_mm - lower_at_surface).abs() < 0.02, "{}", lower.contact_diameter_mm);
        assert!((lower.body_diameter_mm - 0.8).abs() < 0.03, "{}", lower.body_diameter_mm);
        assert!(close(s.tip.socket, upper_socket, 0.05), "{}", s.tip.socket);
        let upper_at_surface = 0.3 + 0.5 * 0.2 / 4.2;
        assert!((s.tip.contact_diameter_mm - upper_at_surface).abs() < 0.02, "{}", s.tip.contact_diameter_mm);
    }

    #[test]
    fn supports_standing_clear_of_the_plate_are_left_out() {
        // The same post as on the plate, raised 3 mm: it has no plate to stand on.
        let model = box_model(Vec3::new(-5.0, -5.0, 13.0), Vec3::new(5.0, 5.0, 18.0));
        let on_plate = support(Vec3::new(1.0, 2.0, 13.0), &[Vec3::new(1.0, 2.0, 11.0), Vec3::new(1.0, 2.0, 0.0)]);
        assert_eq!(reconstruct(&model, &on_plate).supports.len(), 1);
        let raised = support(Vec3::new(1.0, 2.0, 13.0), &[Vec3::new(1.0, 2.0, 11.0), Vec3::new(1.0, 2.0, 3.0)]);
        assert!(reconstruct(&model, &raised).supports.is_empty());
    }

    #[test]
    fn supports_away_from_the_model_and_tips_too_low_for_a_root_are_left_out() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let far = support(Vec3::new(20.0, 0.0, 10.0), &[Vec3::new(20.0, 0.0, 8.0), Vec3::new(20.0, 0.0, 0.0)]);
        assert!(reconstruct(&model, &far).supports.is_empty());

        let low_model = box_model(Vec3::new(-5.0, -5.0, 1.5), Vec3::new(5.0, 5.0, 4.0));
        let low = support(Vec3::new(0.0, 0.0, 1.5), &[Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 0.0)]);
        assert!(reconstruct(&low_model, &low).supports.is_empty());
        assert!(reconstruct(&model, &[]).supports.is_empty());
    }

    #[test]
    fn rebuilt_supports_account_for_supports_but_not_for_model_parts() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut supports = support(Vec3::new(-2.0, 0.0, 10.0), &[Vec3::new(-2.0, 0.0, 8.0), Vec3::new(-2.0, 0.0, 0.0)]);
        supports.extend(support(Vec3::new(2.0, 1.0, 10.0), &[Vec3::new(2.0, 1.0, 8.0), Vec3::new(2.0, 1.0, 0.0)]));
        supports.extend(cuboid(Vec3::new(-6.0, -6.0, 0.0), Vec3::new(6.0, 6.0, 1.0), 4));
        let layout = reconstruct(&model, &supports);
        assert_eq!(layout.supports.len(), 2);
        let coverage = layout.coverage(&supports, 2.0).expect("support surface above the raft");
        assert!(coverage > 0.95, "{coverage}");
        assert!(layout.accounts_for(&supports, 2.0));
        // Counted from the top of a 6 mm raft, the posts above it are still covered.
        assert!(layout.coverage(&supports, 6.0).is_some_and(|c| c > 0.95));

        // Thin straps along the model's side: they rebuild into a stub at most.
        let straps: Vec<Triangle> = [11.0, 13.0]
            .iter()
            .flat_map(|&z| cuboid(Vec3::new(5.0, -4.0, z), Vec3::new(6.0, 4.0, z + 1.0), 2))
            .collect();
        let layout = reconstruct(&model, &straps);
        let coverage = layout.coverage(&straps, 2.0).expect("strap surface");
        assert!(coverage < 0.3, "{coverage}");
        assert!(!layout.accounts_for(&straps, 2.0));

        // Nothing above the raft: nothing to account for.
        let raft = cuboid(Vec3::new(-6.0, -6.0, 0.0), Vec3::new(6.0, 6.0, 1.0), 4);
        let layout = reconstruct(&model, &raft);
        assert_eq!(layout.coverage(&raft, 2.0), None);
        assert!(!layout.accounts_for(&raft, 2.0));
    }

    #[test]
    fn supports_hold_the_model_up_and_pins_through_it_do_not() {
        // Posts under the model meet its underside from below.
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut posts = support(Vec3::new(-2.0, 0.0, 10.0), &[Vec3::new(-2.0, 0.0, 8.0), Vec3::new(-2.0, 0.0, 0.0)]);
        posts.extend(support(Vec3::new(2.0, 1.0, 10.0), &[Vec3::new(2.0, 1.0, 8.0), Vec3::new(2.0, 1.0, 0.0)]));
        let layout = reconstruct(&model, &posts);
        assert_eq!(layout.supports.len(), 2);
        assert!(layout.supports.iter().all(|s| s.tip.from_outside));
        assert_eq!(layout.holding_share(), Some(1.0));

        // Pins from the plate up through a block standing on it, flush with its top: they meet
        // the top face from within.
        let block = box_model(Vec3::new(-5.0, -5.0, 0.0), Vec3::new(5.0, 5.0, 6.0));
        let pins: Vec<Triangle> = [-2.0, 2.0]
            .iter()
            .flat_map(|&x| frustum(Vec3::new(x, 0.0, 0.0), Vec3::new(x, 0.0, 6.0), 0.6, 0.6))
            .collect();
        let layout = reconstruct(&block, &pins);
        assert_eq!(layout.supports.len(), 2);
        assert!(layout.supports.iter().all(|s| !s.tip.from_outside));
        assert_eq!(layout.holding_share(), Some(0.0));
        assert!(!layout.accounts_for(&pins, 2.0));
    }

    #[test]
    fn holding_share_counts_tips_meeting_a_downward_face_from_outside() {
        let support = |contact: Vec3, surface_normal: Vec3, from_outside: bool| SupportPath {
            tip: ContactTip {
                contact,
                surface_normal,
                from_outside,
                socket: contact - Vec3::new(0.0, 0.0, 0.3),
                contact_diameter_mm: 0.3,
                body_diameter_mm: 0.6,
            },
            base: SupportBase::Plate { x: contact.x, y: contact.y, diameter_mm: 3.0 },
            joints: vec![],
            shaft_diameter_mm: 0.6,
        };
        let (down, side, tilted) =
            (Vec3::new(0.0, 0.0, -1.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.99, 0.0, -0.1).normalized());
        let layout = SupportLayout {
            supports: vec![
                support(Vec3::new(0.0, 0.0, 10.0), down, true),
                // A face leaning a little over the support still rests on it.
                support(Vec3::new(1.0, 0.0, 10.0), tilted, true),
                // A peg out of a side face, and a pin meeting a face from inside the model.
                support(Vec3::new(2.0, 0.0, 10.0), side, true),
                support(Vec3::new(3.0, 0.0, 10.0), down, false),
                // Under 1 mm from its root's top: not counted.
                support(Vec3::new(4.0, 0.0, ROOT_TOP_MM + 0.5), side, true),
            ],
            ..SupportLayout::default()
        };
        assert_eq!(layout.holding_share(), Some(0.5));

        let stubs = SupportLayout { supports: layout.supports[4..].to_vec(), ..SupportLayout::default() };
        assert_eq!(stubs.holding_share(), None);
        assert_eq!(SupportLayout::default().holding_share(), None);

        // Struts under a downward face: one standing on a floor, one against a side face, one
        // meeting a floor from inside the model.
        let strut = |lower_normal: Vec3, from_outside: bool| {
            let mut s = support(Vec3::new(0.0, 0.0, 10.0), down, true);
            s.base = SupportBase::Model(ContactTip {
                contact: Vec3::new(0.0, 0.0, 5.0),
                surface_normal: lower_normal,
                from_outside,
                socket: Vec3::new(0.0, 0.0, 5.3),
                contact_diameter_mm: 0.3,
                body_diameter_mm: 0.6,
            });
            s
        };
        let up = Vec3::new(0.0, 0.0, 1.0);
        let struts = |bases: &[(Vec3, bool)]| SupportLayout {
            supports: bases.iter().map(|&(n, outside)| strut(n, outside)).collect(),
            ..SupportLayout::default()
        };
        assert_eq!(struts(&[(up, true)]).holding_share(), Some(1.0));
        assert_eq!(struts(&[(side, true)]).holding_share(), Some(0.0));
        assert_eq!(struts(&[(up, false)]).holding_share(), Some(0.0));
        assert_eq!(struts(&[(up, true), (side, true)]).holding_share(), Some(0.5));
    }

    #[test]
    fn shaft_diameter_reads_the_shaft_past_its_joint_balls_and_foot() {
        // Nine sections of a 1 mm shaft: two at its tapering foot, and five widened by joint
        // balls and merged braces. The lower third of the sections is the shaft itself; the
        // median would be a joint ball, the lowest the foot.
        let widths = vec![1.5, 0.7, 1.0, 1.5, 1.5, 0.7, 1.0, 1.5, 1.5];
        assert_eq!(shaft_diameter(widths, 0.5), 1.0);
        assert_eq!(shaft_diameter(Vec::new(), 0.5), 0.5);
    }

    #[test]
    fn simplify_keeps_the_ends_and_real_bends_only() {
        let line = [
            Vec3::new(0.0, 0.0, 10.0),
            Vec3::new(0.05, 0.0, 9.0),
            Vec3::new(0.0, 0.0, 8.0),
            Vec3::new(2.0, 0.0, 6.0),
            Vec3::new(2.0, 0.0, 0.0),
        ];
        assert_eq!(simplify(&line, SIMPLIFY_MIN_MM), vec![0, 2, 3, 4]);
        assert_eq!(simplify(&line[..2], SIMPLIFY_MIN_MM), vec![0, 1]);
    }

    #[test]
    fn bend_tolerance_grows_with_the_shaft_but_keeps_its_floor() {
        // A 0.35 mm kink: a bend on a thin shaft, slice wander on a thick one.
        let line = [Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.35, 0.0, 5.0), Vec3::new(0.0, 0.0, 0.0)];
        assert_eq!(bends(&line, 0.6).len(), 1);
        assert_eq!(bends(&line, 1.2).len(), 1);
        assert!(bends(&line, 1.6).is_empty());
        // Below the floor the tolerance does not shrink: a thinner shaft drops the same wander.
        let wander = [Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.25, 0.0, 5.0), Vec3::new(0.0, 0.0, 0.0)];
        assert!(bends(&wander, 0.3).is_empty());
        assert_eq!(scaled(0.3, SIMPLIFY_RATIO, SIMPLIFY_MIN_MM), SIMPLIFY_MIN_MM);
        assert!((scaled(2.0, SIMPLIFY_RATIO, SIMPLIFY_MIN_MM) - 0.5).abs() < 1e-6);
    }

    /// The layers of the shaft points a walk from the top blob of `blobs` (a hand graph) takes.
    fn walked_layers(blobs: &[(f32, u32, f32)]) -> Vec<u32> {
        let graph = hand_graph(blobs);
        let top = (0..graph.blobs.len()).max_by_key(|&i| graph.blobs[i].layer).expect("a blob");
        let walk = graph.walk(top as u32, &vec![NONE; graph.blobs.len()], true, |_| true);
        assert!(matches!(walk.end, WalkEnd::Plate));
        walk.nodes.iter().map(|&i| graph.blobs[i as usize].layer).collect()
    }

    #[test]
    fn blunt_tip_flaring_into_its_shaft_is_walked_slice_by_slice() {
        // A tip 0.2 mm across at its top that flares to a 1 mm shaft within half a millimetre:
        // the walked width cannot follow that fast, but each slice is within the slack of it.
        let blobs: Vec<(f32, u32, f32)> =
            (0..=60).map(|layer| (0.0, layer, 0.1 + 0.08 * (60 - layer).min(5) as f32)).collect();
        let layers = walked_layers(&blobs);
        assert_eq!(layers, (0..=60).rev().collect::<Vec<_>>());
    }

    #[test]
    fn one_pinched_slice_barely_narrows_the_walked_shaft() {
        // A 0.8 mm post with one slice pinched to half its width (a seam in the mesh), set 0.1 mm
        // aside below it: the walked width eases only a little towards the pinch, so the post
        // below still reads as the shaft on its own rather than merged with something beside it.
        let blobs: Vec<(f32, u32, f32)> = (0..=60)
            .map(|layer| match layer {
                30 => (0.0, layer, 0.2),
                l if l < 30 => (0.1, layer, 0.4),
                _ => (0.0, layer, 0.4),
            })
            .collect();
        let layers = walked_layers(&blobs);
        assert!((20..=29).all(|l| layers.contains(&l)), "{layers:?}");
    }

    #[test]
    fn thick_post_whose_slices_wander_is_walked_slice_by_slice() {
        // A 2 mm post whose slices' middles wander 0.2 mm from slice to slice: a thick shaft's
        // slices wander further off its line than a thin one's without it bending.
        let blobs: Vec<(f32, u32, f32)> = (0..=60).map(|layer| (0.2 * (layer % 2) as f32, layer, 1.0)).collect();
        let layers = walked_layers(&blobs);
        assert_eq!(layers, (0..=60).rev().collect::<Vec<_>>());
    }

    #[test]
    fn brace_leaning_flatter_than_a_shaft_is_not_picked_up_as_one() {
        // A post ending in a brace that leans 2 mm per mm down to the plate: the walk skips its
        // slices, which never settle into a run it would take for a shaft.
        let mut blobs: Vec<(f32, u32, f32)> = (40..=60).map(|layer| (0.0, layer, 0.4)).collect();
        blobs.extend((0..40).map(|layer| (0.2 * (40 - layer) as f32, layer, 0.4)));
        let layers = walked_layers(&blobs);
        assert!(!layers.iter().any(|l| (30..40).contains(l)), "{layers:?}");
    }

    #[test]
    fn brace_peeling_off_a_post_does_not_pull_its_line_aside() {
        // A 0.8 mm post with a brace peeling off it: for 1.5 mm their slices merge, widening by
        // 0.04 mm a layer towards the brace. The walked width keeps near the post's, so of those
        // slices, centred off the post, only the first few are taken as points on its line.
        let mut blobs: Vec<(f32, u32, f32)> = (0..=60)
            .map(|layer| match layer {
                30..=45 => {
                    let grow = 0.04 * (46 - layer) as f32;
                    (grow / 2.0, layer, 0.4 + grow / 2.0)
                }
                _ => (0.0, layer, 0.4),
            })
            .collect();
        blobs.extend((0..30).map(|layer| (1.0 + 0.1 * (30 - layer) as f32, layer, 0.3)));
        let graph = hand_graph(&blobs);
        let top = (0..graph.blobs.len()).max_by_key(|&i| graph.blobs[i].layer).expect("a blob");
        let walk = graph.walk(top as u32, &vec![NONE; graph.blobs.len()], true, |_| true);
        let xs: Vec<f32> = walk.nodes.iter().map(|&i| graph.blobs[i as usize].x).collect();
        assert!(xs.iter().all(|x| x.abs() < 0.2), "{xs:?}");
    }

    #[test]
    fn thin_shaft_slices_reading_a_little_wide_stay_on_its_line() {
        // A 0.3 mm shaft whose slices read 0.04 mm wider every other layer (a polygon's corners
        // turning past the slice): well within the slack, so every slice is the shaft's own.
        let blobs: Vec<(f32, u32, f32)> =
            (0..=40).map(|layer| (0.0, layer, 0.15 + 0.02 * (layer % 2) as f32)).collect();
        let graph = hand_graph(&blobs);
        let nodes: Vec<u32> = (0..=40).rev().collect();
        assert_eq!(shaft_slices(&graph, &nodes).len(), nodes.len());
    }

    #[test]
    fn branch_ending_a_hair_short_of_its_parent_still_joins_it() {
        // A branch leaning into the side of a trunk, its end a few hundredths of a millimetre
        // short of the trunk's surface (separate shells, as exported): still a branch of it.
        let joins = |end_x: f32| {
            let mut model = box_model(Vec3::new(-2.0, -2.0, 10.0), Vec3::new(2.0, 2.0, 15.0));
            model.extend(box_model(Vec3::new(2.0, -2.0, 9.0), Vec3::new(6.0, 2.0, 15.0)));
            let mut supports =
                support(Vec3::new(0.0, 0.0, 10.0), &[Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, 0.0)]);
            supports.extend(frustum(Vec3::new(4.0, 0.0, 9.2), Vec3::new(4.0, 0.0, 7.0), 0.12, 0.3));
            supports.extend(frustum(Vec3::new(4.0, 0.0, 7.0), Vec3::new(end_x, 0.0, 4.0), 0.3, 0.3));
            let layout = reconstruct(&model, &supports);
            layout.supports.len() == 2 && matches!(layout.supports[1].base, SupportBase::Shaft { parent: 0, .. })
        };
        // Touching the trunk, and with its lowest slice's box a few hundredths of a millimetre
        // from the box of the trunk's slice below it.
        assert!(joins(0.45));
        assert!(joins(0.56));
    }

    #[test]
    fn tip_with_a_straight_neck_at_its_contact_is_not_cut_short() {
        // A tip that keeps its 0.24 mm width for its first 0.4 mm before widening into its cone:
        // the neck stops widening, but is too short to be the whole tip.
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut tip = frustum(Vec3::new(0.0, 0.0, 10.2), Vec3::new(0.0, 0.0, 9.6), 0.12, 0.12);
        tip.extend(frustum(Vec3::new(0.0, 0.0, 9.6), Vec3::new(0.0, 0.0, 8.0), 0.12, 0.4));
        tip.extend(frustum(Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, 0.0), 0.4, 0.4));
        let layout = reconstruct(&model, &tip);

        assert_eq!(layout.supports.len(), 1);
        let socket = layout.supports[0].tip.socket;
        assert!(socket.z < 9.0, "{socket}");
    }

    #[test]
    fn thin_tip_widening_slowly_is_not_cut_short() {
        // A thin tip (0.12 to 0.4 mm over 2.3 mm, below a short neck): it widens by a hair over
        // a fifth of its width, but steadily, so it runs on to where its shaft starts.
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut tip = frustum(Vec3::new(0.0, 0.0, 10.2), Vec3::new(0.0, 0.0, 9.9), 0.06, 0.06);
        tip.extend(frustum(Vec3::new(0.0, 0.0, 9.9), Vec3::new(0.0, 0.0, 7.6), 0.06, 0.2));
        tip.extend(frustum(Vec3::new(0.0, 0.0, 7.6), Vec3::new(0.0, 0.0, 0.0), 0.2, 0.2));
        let layout = reconstruct(&model, &tip);

        assert_eq!(layout.supports.len(), 1);
        let socket = layout.supports[0].tip.socket;
        assert!(socket.z < 8.2, "{socket}");
    }

    #[test]
    fn thick_tip_widening_steadily_is_measured_over_a_longer_run() {
        // A long tip widening from 0.6 to 1.1 mm over 5 mm below a short neck: over a run as long
        // as a thin tip's it barely widens, but over a quarter of its own width it does.
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut tip = frustum(Vec3::new(0.0, 0.0, 10.2), Vec3::new(0.0, 0.0, 9.9), 0.3, 0.3);
        tip.extend(frustum(Vec3::new(0.0, 0.0, 9.9), Vec3::new(0.0, 0.0, 4.9), 0.3, 0.55));
        tip.extend(frustum(Vec3::new(0.0, 0.0, 4.9), Vec3::new(0.0, 0.0, 0.0), 0.55, 0.55));
        let layout = reconstruct(&model, &tip);

        assert_eq!(layout.supports.len(), 1);
        let socket = layout.supports[0].tip.socket;
        assert!(socket.z < 8.0, "{socket}");
    }

    #[test]
    fn thin_tip_flaring_gently_into_its_shaft_ends_where_its_cone_does() {
        // A thin tip whose cone (0.1 to 0.24 mm over 0.6 mm) flares on by 0.06 mm over the next
        // 0.8 mm into its shaft: the tip has stopped widening where its cone ends.
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut tip = frustum(Vec3::new(0.0, 0.0, 10.2), Vec3::new(0.0, 0.0, 9.9), 0.05, 0.05);
        tip.extend(frustum(Vec3::new(0.0, 0.0, 9.9), Vec3::new(0.0, 0.0, 9.3), 0.05, 0.12));
        tip.extend(frustum(Vec3::new(0.0, 0.0, 9.3), Vec3::new(0.0, 0.0, 8.5), 0.12, 0.15));
        tip.extend(frustum(Vec3::new(0.0, 0.0, 8.5), Vec3::new(0.0, 0.0, 0.0), 0.15, 0.15));
        let layout = reconstruct(&model, &tip);

        assert_eq!(layout.supports.len(), 1);
        let socket = layout.supports[0].tip.socket;
        assert!(socket.z > 9.0, "{socket}");
    }

    #[test]
    fn hair_thin_shaft_rebuilds_at_the_thinnest_rod() {
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut post = frustum(Vec3::new(0.0, 0.0, 10.2), Vec3::new(0.0, 0.0, 8.5), 0.05, 0.1);
        post.extend(frustum(Vec3::new(0.0, 0.0, 8.5), Vec3::new(0.0, 0.0, 0.0), 0.1, 0.1));
        let layout = reconstruct(&model, &post);

        assert_eq!(layout.supports.len(), 1);
        assert_eq!(layout.supports[0].shaft_diameter_mm, MIN_ROD_DIAMETER_MM);
    }

    #[test]
    fn tip_contact_is_read_below_its_rounded_end() {
        // A tip touching the model with a rounded end: its top slices cut through the rounding,
        // so the contact is as wide as the tip just below it.
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut tip = tube(&[
            (Vec3::new(0.0, 0.0, 10.0), 0.05),
            (Vec3::new(0.0, 0.0, 9.9), 0.17),
            (Vec3::new(0.0, 0.0, 9.8), 0.23),
            (Vec3::new(0.0, 0.0, 9.7), 0.25),
            (Vec3::new(0.0, 0.0, 9.0), 0.3),
            (Vec3::new(0.0, 0.0, 8.0), 0.4),
        ]);
        tip.extend(frustum(Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, 0.0), 0.4, 0.4));
        let layout = reconstruct(&model, &tip);

        assert_eq!(layout.supports.len(), 1);
        let d = layout.supports[0].tip.contact_diameter_mm;
        assert!(d >= 0.35, "{d}");
    }

    #[test]
    fn blunt_post_against_the_model_keeps_a_contact_of_at_most_one_and_a_half_millimetres() {
        // A 2.4 mm post standing against the model with no tip cone.
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let post = frustum(Vec3::new(0.0, 0.0, 10.2), Vec3::new(0.0, 0.0, 0.0), 1.2, 1.2);
        let layout = reconstruct(&model, &post);

        assert_eq!(layout.supports.len(), 1);
        let d = layout.supports[0].tip.contact_diameter_mm;
        assert!((d - MAX_CONTACT_DIAMETER_MM).abs() < 1e-4, "{d}");
    }

    #[test]
    fn short_strut_tapering_from_both_ends_splits_between_its_two_tips() {
        // A strut 4 mm long between two parts of a model, one shell tapering from 1 mm in its
        // middle to 0.24 mm at each end: each tip takes up to 0.4 of it.
        let mut model = box_model(Vec3::new(-4.0, -4.0, 0.0), Vec3::new(4.0, 4.0, 4.0));
        model.extend(box_model(Vec3::new(-4.0, -4.0, 8.0), Vec3::new(4.0, 4.0, 12.0)));
        let strut = tube(&[
            (Vec3::new(0.0, 0.0, 8.2), 0.12),
            (Vec3::new(0.0, 0.0, 6.0), 0.5),
            (Vec3::new(0.0, 0.0, 3.8), 0.12),
        ]);
        let layout = reconstruct(&model, &strut);

        assert_eq!(layout.supports.len(), 1);
        let s = &layout.supports[0];
        let SupportBase::Model(lower) = &s.base else { panic!("{:?}", s.base) };
        // The upper tip ends within 0.4 of the strut's length of its contact (1.6 mm from 8.0).
        assert!((6.3..=6.6).contains(&s.tip.socket.z), "{}", s.tip.socket);
        assert!(lower.socket.z < s.tip.socket.z, "{}", lower.socket);
    }

    #[test]
    fn tip_whose_socket_sits_at_the_root_top_has_no_shaft_and_is_left_out() {
        let model = box_model(Vec3::new(-5.0, -5.0, 3.0), Vec3::new(5.0, 5.0, 8.0));
        let low = |socket_z: f32| {
            let mut supports = frustum(Vec3::new(0.0, 0.0, 3.2), Vec3::new(0.0, 0.0, socket_z), 0.12, 0.4);
            supports.extend(frustum(Vec3::new(0.0, 0.0, socket_z), Vec3::new(0.0, 0.0, 0.0), 0.4, 0.4));
            reconstruct(&model, &supports).supports.len()
        };
        // A socket 0.1 mm above the root top leaves too short a shaft; 0.4 mm is enough.
        assert_eq!(low(2.1), 0);
        assert_eq!(low(2.4), 1);
    }

    #[test]
    fn shaft_thickening_below_a_step_keeps_its_lower_slices_on_its_line() {
        // A shaft 0.6 mm across for 2.5 mm that steps out to 1 mm for the 2 mm below: each slice
        // is measured against those near its own height, not the shaft's median width.
        let blobs: Vec<(f32, u32, f32)> =
            (0..45).map(|layer| (0.0, layer, if layer < 20 { 0.5 } else { 0.3 })).collect();
        let graph = hand_graph(&blobs);
        let nodes: Vec<u32> = (0..45).rev().collect();
        let kept = shaft_slices(&graph, &nodes);
        assert!((0..14).all(|i| kept.contains(&i)), "{kept:?}");
    }

    #[test]
    fn second_top_merging_into_its_tip_within_a_millimetre_is_not_a_support() {
        // A plate support whose tip has a second, thinner top 0.3 mm beside its own, merging into
        // the tip 0.9 mm below the model.
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut supports = support(Vec3::new(0.0, 0.0, 10.0), &[Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, 0.0)]);
        supports.extend(frustum(Vec3::new(0.3, 0.0, 10.1), Vec3::new(0.1, 0.0, 9.1), 0.1, 0.15));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 1, "{:?}", layout.supports);
    }

    #[test]
    fn knot_reach_grows_with_the_brace_but_keeps_its_floor() {
        // Where a brace merges into a shaft lies further out on a thicker brace.
        assert_eq!(knot_reach_mm(0.6), KNOT_REACH_MIN_MM);
        assert!((knot_reach_mm(2.0) - 0.84).abs() < 1e-5);
    }

    #[test]
    fn brace_ending_on_a_third_support_beside_its_host_does_not_run_through_it() {
        // A bar between two supports whose upper end lies on a third support's centre line,
        // where three shafts meet: only its middle third is tested, which passes clear.
        let shafts = [(Vec3::new(3.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 10.0), 0.4)];
        let shaft_hosts = [((2, None), true)];
        let crosses = |p: Vec3, q: Vec3| through_shaft(&shafts, &shaft_hosts, [(0, None), (1, None)], p, q);
        assert!(!crosses(Vec3::new(0.0, 0.0, 3.0), Vec3::new(3.0, 0.0, 6.0)));
        assert!(crosses(Vec3::new(1.0, 0.0, 3.0), Vec3::new(5.0, 0.0, 7.0)));
    }

    #[test]
    fn slice_of_a_thin_shaft_a_little_wider_than_it_is_not_merged_with_a_brace() {
        // A 0.3 mm shaft with one slice 0.4 mm across centred on it: a quarter wider plus the
        // slack, so the shaft itself rather than the shaft merged with a bar beside it.
        let blobs: Vec<(f32, u32, f32)> =
            (0..=40).map(|layer| (0.0, layer, if layer == 20 { 0.2 } else { 0.15 })).collect();
        let graph = hand_graph(&blobs);
        let at = |layer: u32| graph.blobs.iter().position(|b| b.layer == layer).expect("a blob") as u32;
        let nodes: Vec<u32> = (21..=40).rev().map(at).collect();
        assert!(graph.merged_descent_slope(at(20), &nodes, None, 0.15).is_none());
        assert!(graph.merged_descent_slope(at(20), &nodes, None, 0.12).is_some());
    }

    #[test]
    fn brace_running_on_through_a_middle_post_is_two_braces() {
        // One straight bar from high on the first of three posts in a row down to the third,
        // through the middle one: a brace from each outer post to the middle one.
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(10.0, 5.0, 15.0));
        let mut supports = Vec::new();
        for x in [0.0, 3.0, 6.0] {
            supports.extend(support(Vec3::new(x, 0.0, 10.0), &[Vec3::new(x, 0.0, 8.0), Vec3::new(x, 0.0, 0.0)]));
        }
        supports.extend(frustum(Vec3::new(0.0, 0.0, 7.0), Vec3::new(6.0, 0.0, 2.0), 0.3, 0.3));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.braces.len(), 2, "{:?}", layout.braces);
        for brace in &layout.braces {
            assert!((brace.upper.point.x - brace.lower.point.x).abs() < 4.0, "{brace:?}");
        }
    }

    #[test]
    fn leg_passing_a_post_just_above_its_root_is_one_kickstand() {
        // A leg leaning 1.9 mm per mm from the trunk 6 mm up out to the plate, passing through
        // another post 0.85 mm above the post's root top: it only meets the root there, so it
        // stays one kickstand under the trunk, with a brace from the post to the trunk.
        let model = box_model(Vec3::new(-6.0, -6.0, 12.0), Vec3::new(14.0, 6.0, 16.0));
        let mut supports = support(Vec3::new(0.0, 0.0, 12.0), &[Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, 0.0)]);
        supports.extend(frustum(Vec3::new(0.0, 0.0, 6.0), Vec3::new(11.4, 0.0, 0.0), 0.3, 0.3));
        let px = 1.9 * (6.0 - 2.85);
        supports.extend(support(Vec3::new(px, 0.0, 12.0), &[Vec3::new(px, 0.0, 10.0), Vec3::new(px, 0.0, 0.0)]));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.kickstands.len(), 1, "{:?}", layout.kickstands);
        let leg = &layout.kickstands[0];
        assert_eq!(leg.support, 0);
        assert!(close(leg.knot, Vec3::new(0.0, 0.0, 6.0), 0.3), "{}", leg.knot);
        assert!(close(leg.foot, Vec3::new(7.6, 0.0, ROOT_TOP_MM), 0.3), "{}", leg.foot);
    }

    #[test]
    fn brace_leaving_a_hub_where_two_supports_meet_is_one_brace() {
        // Two posts 0.7 mm apart, their shafts merged into one, with a leg leaning from between
        // them 6 mm up out to the plate and a brace from there down to a third post: the brace
        // touches both posts at that hub, but is one brace.
        let model = box_model(Vec3::new(-6.0, -6.0, 12.0), Vec3::new(6.0, 6.0, 16.0));
        let mut supports = Vec::new();
        for x in [0.0, 0.7, -4.0] {
            supports.extend(support(Vec3::new(x, 0.0, 12.0), &[Vec3::new(x, 0.0, 10.0), Vec3::new(x, 0.0, 0.0)]));
        }
        supports.extend(frustum(Vec3::new(0.35, 0.0, 6.0), Vec3::new(3.85, 0.0, 0.0), 0.3, 0.3));
        supports.extend(frustum(Vec3::new(0.35, 0.0, 6.0), Vec3::new(-4.0, 0.0, 3.0), 0.3, 0.3));
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.braces.len(), 1, "{:?}", layout.braces);
        assert_eq!(layout.kickstands.len(), 1, "{:?}", layout.kickstands);
    }

    #[test]
    fn raft_rising_a_little_above_its_height_is_not_counted_against_coverage() {
        // Two posts on a raft whose top stands 0.3 mm above the raft height the split used: its
        // top face is raft, not support to rebuild.
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let mut supports = support(Vec3::new(-2.0, 0.0, 10.0), &[Vec3::new(-2.0, 0.0, 8.0), Vec3::new(-2.0, 0.0, 0.0)]);
        supports.extend(support(Vec3::new(2.0, 1.0, 10.0), &[Vec3::new(2.0, 1.0, 8.0), Vec3::new(2.0, 1.0, 0.0)]));
        supports.extend(cuboid(Vec3::new(-6.0, -6.0, 0.0), Vec3::new(6.0, 6.0, 2.3), 4));
        let layout = reconstruct(&model, &supports);
        assert_eq!(layout.supports.len(), 2);
        let coverage = layout.coverage(&supports, 2.0).expect("support surface above the raft");
        assert!(coverage > 0.95, "{coverage}");
    }

    #[test]
    fn root_is_three_shafts_wide_within_its_limits() {
        // Upright posts of 0.5, 1.2 and 2.4 mm: roots three times as wide, but never under 2 mm
        // (a thin post's root would not hold it to the plate) or over 6 mm.
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let root = |radius: f32| {
            let contact = Vec3::new(0.0, 0.0, 10.0);
            let socket = Vec3::new(0.0, 0.0, 8.0);
            let mut supports = frustum(contact + Vec3::new(0.0, 0.0, 0.2), socket, 0.12, radius);
            supports.extend(frustum(socket, Vec3::new(0.0, 0.0, 0.0), radius, radius));
            let layout = reconstruct(&model, &supports);
            assert_eq!(layout.supports.len(), 1);
            let SupportBase::Plate { diameter_mm, .. } = layout.supports[0].base else { panic!() };
            (layout.supports[0].shaft_diameter_mm, diameter_mm)
        };
        let (shaft, diameter) = root(0.25);
        assert!(shaft * 3.0 < 1.8 && (diameter - 2.0).abs() < 1e-4, "{shaft} {diameter}");
        let (shaft, diameter) = root(0.6);
        assert!((diameter - shaft * 3.0).abs() < 1e-4 && (3.3..=3.9).contains(&diameter), "{shaft} {diameter}");
        let (shaft, diameter) = root(1.2);
        assert!(shaft * 3.0 > 6.5 && (diameter - 6.0).abs() < 1e-4, "{shaft} {diameter}");
    }

    #[test]
    fn thick_supports_rebuild_without_extra_joints() {
        // A thick 2 mm post with joint balls, wandering about 0.4 mm off a straight line (less
        // than a fifth of its width): one straight trunk, where a thin shaft would bend.
        let model = box_model(Vec3::new(-5.0, -5.0, 10.0), Vec3::new(5.0, 5.0, 15.0));
        let contact = Vec3::new(1.0, 2.0, 10.0);
        let path = [Vec3::new(1.0, 2.0, 8.0), Vec3::new(1.55, 2.0, 4.5), Vec3::new(1.0, 2.0, 0.0)];
        let mut supports = frustum(contact + Vec3::new(0.0, 0.0, 0.2), path[0], 0.3, 1.0);
        for w in path.windows(2) {
            supports.extend(frustum(w[0], w[1], 1.0, 1.0));
            supports.extend(frustum(w[0] + Vec3::new(0.0, 0.0, 0.9), w[0] - Vec3::new(0.0, 0.0, 0.9), 1.2, 1.2));
        }
        let layout = reconstruct(&model, &supports);

        assert_eq!(layout.supports.len(), 1);
        let s = &layout.supports[0];
        assert!((1.8..=2.2).contains(&s.shaft_diameter_mm), "{}", s.shaft_diameter_mm);
        assert!(s.joints.is_empty(), "{:?}", s.joints);
    }
}
