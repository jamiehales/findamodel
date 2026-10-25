export const CANVAS_WIDTH_MM = 228;
export const CANVAS_HEIGHT_MM = 128;
export const VIEW_TOP_MARGIN_MM = 12;
export const PX_PER_MM = 4;
export const CANVAS_WIDTH_PX = CANVAS_WIDTH_MM * PX_PER_MM;
export const CANVAS_HEIGHT_PX = CANVAS_HEIGHT_MM * PX_PER_MM;
export const VIEW_TOP_MARGIN_PX = VIEW_TOP_MARGIN_MM * PX_PER_MM;
export const VIEW_HEIGHT_PX = CANVAS_HEIGHT_PX + VIEW_TOP_MARGIN_PX;

export const WALL_THICKNESS = 200;
export const SPEED_THRESH = 0.005;
export const ANGULAR_THRESH = 0.003;
export const SETTLE_FRAMES = 90; // ~1.5 s at 60 fps
export const LAYOUT_LOCALSTORAGE_KEY = 'findamodel.printingListLayout';
export const PAUSE_ON_DRAG_LOCALSTORAGE_KEY = 'findamodel.printingListPauseOnDrag';
export const SHOW_LABELS_LOCALSTORAGE_KEY = 'findamodel.printingListShowLabels';
export const PUSH_TO_EDGES_LOCALSTORAGE_KEY = 'findamodel.printingListPushToEdges';
export const DEBUG_PHYSICS_WIREFRAME = false;
export const PHYSICS_BORDER_PADDING_PX = 0;

export const GRAVITY_ACCEL = 20;
// "Push to edges" holds each model in its slot on the plate edge with a damped spring
// (acceleration per unit of distance, and per unit of speed). Models that don't fit around
// the edges are pushed outward at the capped acceleration.
export const EDGE_SPRING = 4;
export const EDGE_DAMPING = 2.5;
export const EDGE_MAX_ACCEL = GRAVITY_ACCEL * 0.4;
// How far a model's top can poke above the plate and still count as on it, so the top
// edge holds a model resting against it.
export const PLATE_ENTRY_TOLERANCE_PX = 5;
// Models that join the plate at its centre while pushing to edges wait for the middle to
// clear; one still waiting after this many frames is parked above the plate instead.
export const CENTRE_RELEASE_MAX_WAIT_FRAMES = 120;

// Physics body inflation - when two bodies touch they have BODY_GAP_MM visual gap.
export const BODY_GAP_MM = 2;
export const BODY_MARGIN_PX = (BODY_GAP_MM / 2) * PX_PER_MM;

// planck.js (Box2D) is tuned for objects 0.1–10 m.  1 physics unit = 100 px.
export const PHYSICS_SCALE = 100;
export const toPhysics = (px: number): number => px / PHYSICS_SCALE;
export const toPixels = (p: number): number => p * PHYSICS_SCALE;
export const CANVAS_WIDTH_PHYS = toPhysics(CANVAS_WIDTH_PX);
export const CANVAS_HEIGHT_PHYS = toPhysics(CANVAS_HEIGHT_PX);

// Three categories allow per-fixture routing:
//   WALL   fixtures only touch BORDER fixtures  (keeps models on the plate)
//   OBJECT fixtures only touch other OBJECT fixtures (object-object spacing)
//   BORDER fixtures only touch WALL   fixtures  (plate edge containment)
export const CAT_WALL = 0x0001;
export const CAT_OBJECT = 0x0002;
export const CAT_BORDER = 0x0004;

// Hull colors from the mostlymaking data palette and its lighter text shades.
export const PALETTE = [
  0x94a7cc, // slate
  0xc9a45c, // brass
  0x8fb894, // sage
  0xb892c0, // heather
  0xe0909b, // rose (primary text)
  0xe0cfa6, // parchment (secondary text)
  0xa9a5b0, // muted
  0xef9a96, // vermilion (destructive text)
];
