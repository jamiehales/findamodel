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
export const DEBUG_PHYSICS_WIREFRAME = false;
export const PHYSICS_BORDER_PADDING_PX = 0;

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

export const PALETTE = [
  0x818cf8, // indigo
  0x34d399, // emerald
  0xfb923c, // orange
  0xf472b6, // pink
  0xa78bfa, // violet
  0x38bdf8, // sky
  0xfbbf24, // amber
  0xf87171, // rose
];
