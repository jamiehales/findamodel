import { useEffect, useMemo, useRef, useState } from 'react';
import * as PIXI from 'pixi.js';
import Matter from 'matter-js';
import {
  Stack,
  Typography,
  Button,
  MenuItem,
  Select,
  Checkbox,
  FormGroup,
  FormControlLabel,
  FormControl,
  InputLabel,
} from '@mui/material';
import type { Model, SpawnType, HullMode } from '../lib/api';

// ── Constants ─────────────────────────────────────────────────────────────────

const CANVAS_WIDTH_MM = 228;
const CANVAS_HEIGHT_MM = 128;
const VIEW_TOP_MARGIN_MM = 12;
const PX_PER_MM = 4;
const CANVAS_WIDTH_PX = CANVAS_WIDTH_MM * PX_PER_MM;
const CANVAS_HEIGHT_PX = CANVAS_HEIGHT_MM * PX_PER_MM;
const VIEW_TOP_MARGIN_PX = VIEW_TOP_MARGIN_MM * PX_PER_MM;
const VIEW_HEIGHT_PX = CANVAS_HEIGHT_PX + VIEW_TOP_MARGIN_PX;

const WALL_THICKNESS = 200;
const SPEED_THRESH = 0.12;
const ANGULAR_THRESH = 0.008;
const SETTLE_FRAMES = 90; // ~1.5 s at 60 fps
export const LAYOUT_LOCALSTORAGE_KEY = 'findamodel.printingListLayout';
const PAUSE_ON_DRAG_LOCALSTORAGE_KEY = 'findamodel.printingListPauseOnDrag';
const DEBUG_PHYSICS_WIREFRAME = false;

// Physics body inflation — when two bodies touch they have BODY_GAP_MM visual gap.
const BODY_GAP_MM = 2;
const BODY_MARGIN_PX = (BODY_GAP_MM / 2) * PX_PER_MM;

const PALETTE = [
  0x818cf8, // indigo
  0x34d399, // emerald
  0xfb923c, // orange
  0xf472b6, // pink
  0xa78bfa, // violet
  0x38bdf8, // sky
  0xfbbf24, // amber
  0xf87171, // rose
];

// ── Types ─────────────────────────────────────────────────────────────────────

interface Vec2 {
  x: number;
  y: number;
}

interface SavedLayout {
  itemsKey: string;
  positions: {
    modelId: string;
    instanceIndex: number;
    xMm: number;
    yMm: number;
    angle: number;
  }[];
}

interface Entry {
  body: Matter.Body;
  gfx: PIXI.Graphics;
  label: PIXI.Text;
  modelId: string;
  instanceIndex: number;
  color: number;
  /** Original (non-inflated) hull vertices in body-local space, for rendering. */
  visualLocalVerts: Vec2[];
}

interface ModelFootprintMetrics {
  bodyHullJson: string | null;
  localVerts: Vec2[] | null;
  footprintAreaPx2: number;
  boundingWidthPx: number;
  boundingHeightPx: number;
  boundingAreaPx2: number;
}

interface SpawnPlanItem {
  model: Model;
  inst: number;
  spawnX: number;
  metrics: ModelFootprintMetrics;
}

interface Props {
  models: Model[];
  items: Record<string, number>;
  spawnOrder: SpawnType;
  hullMode: HullMode;
  onSpawnOrderChange?: (next: SpawnType) => void;
  onHullModeChange?: (next: HullMode) => void;
  onPausedChange?: (paused: boolean) => void;
}

// ── Hull helpers ──────────────────────────────────────────────────────────────

/**
 * Parse a JSON hull string ("[[x,z],[x,z],...]") into local-space pixel
 * vertices centred at the origin
 */
function parseHullLocalPx(hullJson: string | null): Vec2[] | null {
  if (!hullJson) return null;
  try {
    const raw: [number, number][] = JSON.parse(hullJson);
    if (raw.length < 3) return null;

    // Hull coords are [x, z] in model space (assumed mm for 3-D printing STLs).
    // Map z → canvas y.
    const pts = raw.map(([x, z]): Vec2 => ({ x: x * PX_PER_MM, y: z * PX_PER_MM }));

    // Centre at the polygon area centroid (shoelace formula) so it matches
    // what Matter.js uses internally, minimising the shift fromVertices applies.
    let area = 0;
    let cx = 0;
    let cy = 0;
    for (let i = 0, n = pts.length; i < n; i++) {
      const a = pts[i],
        b = pts[(i + 1) % n];
      const cross = a.x * b.y - b.x * a.y;
      area += cross;
      cx += (a.x + b.x) * cross;
      cy += (a.y + b.y) * cross;
    }
    area /= 2;
    cx /= 6 * area;
    cy /= 6 * area;
    const centred = pts.map((p) => ({ x: p.x - cx, y: p.y - cy }));

    return centred;
  } catch {
    return null;
  }
}

function getSpawnHullJson(model: Model, hullMode: HullMode): string | null {
  return hullMode === 'sansRaft' ? (model.convexSansRaftHull ?? model.convexHull) : model.convexHull;
}

function getFillEstimateHullJson(model: Model, hullMode: HullMode): string | null {
  return model.concaveHull ?? getSpawnHullJson(model, hullMode);
}

// ── Geometry helpers ──────────────────────────────────────────────────────────

function polygonArea(verts: Vec2[]): number {
  if (verts.length < 3) return 0;

  let area = 0;
  for (let i = 0; i < verts.length; i++) {
    const a = verts[i];
    const b = verts[(i + 1) % verts.length];
    area += a.x * b.y - b.x * a.y;
  }

  return Math.abs(area) / 2;
}

function getBounds(verts: Vec2[]): { minX: number; maxX: number; minY: number; maxY: number } {
  let minX = Infinity;
  let maxX = -Infinity;
  let minY = Infinity;
  let maxY = -Infinity;

  for (const v of verts) {
    minX = Math.min(minX, v.x);
    maxX = Math.max(maxX, v.x);
    minY = Math.min(minY, v.y);
    maxY = Math.max(maxY, v.y);
  }

  return { minX, maxX, minY, maxY };
}

function getRectLocalVerts(model: Model): Vec2[] {
  const w = Math.max((model.dimensionXMm ?? 20) * PX_PER_MM, 16);
  const h = Math.max((model.dimensionZMm ?? 20) * PX_PER_MM, 16);
  const hw = w / 2;
  const hh = h / 2;

  return [
    { x: -hw, y: -hh },
    { x: hw, y: -hh },
    { x: hw, y: hh },
    { x: -hw, y: hh },
  ];
}

function getModelFootprintMetrics(model: Model, hullMode: HullMode): ModelFootprintMetrics {
  const bodyHullJson = getSpawnHullJson(model, hullMode);
  const localVerts = parseHullLocalPx(bodyHullJson);
  const rectLocalVerts = getRectLocalVerts(model);
  const boundingVerts = localVerts && localVerts.length >= 3 ? localVerts : rectLocalVerts;
  const fillVerts = parseHullLocalPx(getFillEstimateHullJson(model, hullMode)) ?? boundingVerts;
  const bounds = getBounds(boundingVerts);
  const boundingWidthPx = Math.max(bounds.maxX - bounds.minX, 16);
  const boundingHeightPx = Math.max(bounds.maxY - bounds.minY, 16);

  return {
    bodyHullJson,
    localVerts,
    footprintAreaPx2: polygonArea(fillVerts),
    boundingWidthPx,
    boundingHeightPx,
    boundingAreaPx2: boundingWidthPx * boundingHeightPx,
  };
}

function getRandomSpawnX(): number {
  return CANVAS_WIDTH_PX * 0.2 + Math.random() * CANVAS_WIDTH_PX * 0.6;
}

function clampSpawnX(spawnX: number, widthPx: number): number {
  const margin = widthPx / 2 + BODY_MARGIN_PX + 8;
  return Math.min(CANVAS_WIDTH_PX - margin, Math.max(margin, spawnX));
}

function compareBySizeDesc(a: ModelFootprintMetrics, b: ModelFootprintMetrics): number {
  return b.boundingAreaPx2 - a.boundingAreaPx2 || b.footprintAreaPx2 - a.footprintAreaPx2;
}

function buildSpawnPlan(
  models: Model[],
  items: Record<string, number>,
  spawnOrder: SpawnType,
  hullMode: HullMode,
): SpawnPlanItem[] {
  const baseSequence: Array<{ model: Model; inst: number; metrics: ModelFootprintMetrics }> = [];

  for (const model of models) {
    const qty = items[model.id] ?? 0;
    if (qty <= 0) continue;

    const metrics = getModelFootprintMetrics(model, hullMode);
    for (let inst = 0; inst < qty; inst++) {
      baseSequence.push({ model, inst, metrics });
    }
  }

  if (spawnOrder === 'random') {
    for (let i = baseSequence.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [baseSequence[i], baseSequence[j]] = [baseSequence[j], baseSequence[i]];
    }
  }

  if (spawnOrder !== 'largestFirstFillGaps') {
    return baseSequence.map(({ model, inst, metrics }) => ({
      model,
      inst,
      metrics,
      spawnX: clampSpawnX(getRandomSpawnX(), metrics.boundingWidthPx),
    }));
  }

  const remaining = [...baseSequence].sort(
    (a, b) =>
      compareBySizeDesc(a.metrics, b.metrics) ||
      a.model.name.localeCompare(b.model.name) ||
      a.inst - b.inst,
  );

  const plan: SpawnPlanItem[] = [];
  while (remaining.length > 0) {
    const largest = remaining.shift()!;
    const anchorX = clampSpawnX(getRandomSpawnX(), largest.metrics.boundingWidthPx);

    plan.push({
      model: largest.model,
      inst: largest.inst,
      metrics: largest.metrics,
      spawnX: anchorX,
    });

    let remainingGapArea = Math.max(0, largest.metrics.boundingAreaPx2 - largest.metrics.footprintAreaPx2);
    if (remainingGapArea <= 0) continue;

    const fillers = remaining
      .filter(
        (candidate) =>
          candidate.metrics.footprintAreaPx2 > 0
          && candidate.metrics.boundingAreaPx2 < largest.metrics.boundingAreaPx2,
      )
      .sort(
        (a, b) =>
          a.metrics.footprintAreaPx2 - b.metrics.footprintAreaPx2
          || a.metrics.boundingAreaPx2 - b.metrics.boundingAreaPx2
          || a.model.name.localeCompare(b.model.name)
          || a.inst - b.inst,
      );

    for (const filler of fillers) {
      if (filler.metrics.footprintAreaPx2 > remainingGapArea) continue;

      const fillerIndex = remaining.indexOf(filler);
      if (fillerIndex < 0) continue;

      remaining.splice(fillerIndex, 1);
      remainingGapArea -= filler.metrics.footprintAreaPx2;

      plan.push({
        model: filler.model,
        inst: filler.inst,
        metrics: filler.metrics,
        spawnX: clampSpawnX(anchorX, filler.metrics.boundingWidthPx),
      });
    }
  }

  return plan;
}

/**
 * Expand each vertex outward by `amount` px along its vertex normal.
 * The vertex normal is the average of the outward-facing normals of the two
 * adjacent edges, making this a true parallel offset for convex hulls.
 * Winding-order agnostic: the result is flipped to face away from origin when
 * needed (verts must be centred at origin, as parseHullLocalPx guarantees).
 */
function inflateVerts(verts: Vec2[], amount: number): Vec2[] {
  const n = verts.length;
  return verts.map((v, i) => {
    const prev = verts[(i - 1 + n) % n];
    const next = verts[(i + 1) % n];

    // Perpendicular (rotated 90° CW) to each adjacent edge.
    const nx = v.y - prev.y + (next.y - v.y);
    const ny = -(v.x - prev.x) + -(next.x - v.x);

    const len = Math.sqrt(nx * nx + ny * ny);
    if (len < 0.001) return v;

    // Normalise; flip if pointing toward origin rather than away from it.
    const sign = nx * v.x + ny * v.y < 0 ? -1 : 1;
    return { x: v.x + ((sign * nx) / len) * amount, y: v.y + ((sign * ny) / len) * amount };
  });
}

// ── Body factory helpers ──────────────────────────────────────────────────────

const BODY_MASS = 0.01;

const BODY_OPTIONS: Matter.IChamferableBodyDefinition = {
  restitution: 0.05,
  friction: 0.5,
  frictionAir: 0.025,
};

function makePolygonBody(cx: number, cy: number, localVerts: Vec2[]): Matter.Body {
  // fromVertices centres the body at (cx, cy) using the centroid of the passed
  // vertices. Since localVerts are already centred at origin, their centroid ≈ 0,
  // and Matter.js will correctly place the body at (cx, cy).
  const body = Matter.Bodies.fromVertices(0, 0, [localVerts as Matter.Vector[]], BODY_OPTIONS);
  Matter.Body.setMass(body, BODY_MASS);
  Matter.Body.setPosition(body, { x: cx, y: cy });
  return body;
}

/**
 * Returns the physics body (inflated by BODY_MARGIN_PX per side), the visual
 * local vertices (original size), and the overlap-detection vertices (inflated
 * by BODY_OVERLAP_PX per side).
 */
function makeRectBody(
  model: Model,
  cx: number,
  cy: number,
): { body: Matter.Body; visualLocalVerts: Vec2[] } {
  // TODO: There is a minimum body size set here, check what we want to happen if this condition is met
  const rectLocalVerts = getRectLocalVerts(model);
  const bounds = getBounds(rectLocalVerts);
  const w = bounds.maxX - bounds.minX;
  const h = bounds.maxY - bounds.minY;
  const body = Matter.Bodies.rectangle(
    cx,
    cy,
    w + BODY_MARGIN_PX * 2,
    h + BODY_MARGIN_PX * 2,
    BODY_OPTIONS,
  );
  return { body, visualLocalVerts: rectLocalVerts };
}

// ── Color helpers ─────────────────────────────────────────────────────────────

function darkenColor(color: number, factor: number): number {
  const r = Math.round(((color >> 16) & 0xff) * factor);
  const g = Math.round(((color >> 8) & 0xff) * factor);
  const b = Math.round((color & 0xff) * factor);
  return (r << 16) | (g << 8) | b;
}

// ── Overlap helpers ───────────────────────────────────────────────────────────

/** Transform local-space vertices to world space using a body's pose. */
function toWorldVerts(body: Matter.Body, localVerts: Vec2[]): Vec2[] {
  const cos = Math.cos(body.angle);
  const sin = Math.sin(body.angle);
  const { x: px, y: py } = body.position;
  return localVerts.map((v) => ({
    x: v.x * cos - v.y * sin + px,
    y: v.x * sin + v.y * cos + py,
  }));
}

/**
 * Separating Axis Theorem overlap test for two convex polygons.
 * Returns true if they overlap (no separating axis found).
 */
function satOverlap(a: Vec2[], b: Vec2[]): boolean {
  for (const poly of [a, b]) {
    const n = poly.length;
    for (let i = 0; i < n; i++) {
      const p1 = poly[i];
      const p2 = poly[(i + 1) % n];
      const nx = -(p2.y - p1.y);
      const ny = p2.x - p1.x;
      let minA = Infinity,
        maxA = -Infinity;
      let minB = Infinity,
        maxB = -Infinity;
      for (const v of a) {
        const d = v.x * nx + v.y * ny;
        minA = Math.min(minA, d);
        maxA = Math.max(maxA, d);
      }
      for (const v of b) {
        const d = v.x * nx + v.y * ny;
        minB = Math.min(minB, d);
        maxB = Math.max(maxB, d);
      }
      if (maxA < minB || maxB < minA) return false;
    }
  }
  return true;
}

/** Returns the set of body IDs whose overlap-detection hulls intersect any other entry. */
function computeOverlapping(entries: Entry[]): Set<number> {
  const overlapping = new Set<number>();
  const worldVerts = entries.map((e) => toWorldVerts(e.body, e.visualLocalVerts));
  for (let i = 0; i < entries.length; i++) {
    for (let j = i + 1; j < entries.length; j++) {
      if (satOverlap(worldVerts[i], worldVerts[j])) {
        overlapping.add(entries[i].body.id);
        overlapping.add(entries[j].body.id);
      }
    }
  }
  return overlapping;
}

/** Visual bounds check (plate-space): true when any rendered vertex is outside plate rectangle. */
function isOutOfBounds(body: Matter.Body, visualLocalVerts: Vec2[]): boolean {
  if (!visualLocalVerts.length) return false;
  const worldVerts = toWorldVerts(body, visualLocalVerts);
  return worldVerts.some(
    (v) => v.x < 0 || v.x > CANVAS_WIDTH_PX || v.y < 0 || v.y > CANVAS_HEIGHT_PX,
  );
}

function drawDottedRect(
  gfx: PIXI.Graphics,
  x: number,
  y: number,
  w: number,
  h: number,
  dashPx: number,
  gapPx: number,
) {
  const drawDashedLine = (x1: number, y1: number, x2: number, y2: number) => {
    const dx = x2 - x1;
    const dy = y2 - y1;
    const len = Math.hypot(dx, dy);
    if (len <= 0) return;

    const ux = dx / len;
    const uy = dy / len;
    let t = 0;
    while (t < len) {
      const segStart = t;
      const segEnd = Math.min(t + dashPx, len);
      gfx.moveTo(x1 + ux * segStart, y1 + uy * segStart);
      gfx.lineTo(x1 + ux * segEnd, y1 + uy * segEnd);
      t += dashPx + gapPx;
    }
  };

  drawDashedLine(x, y, x + w, y);
  drawDashedLine(x + w, y, x + w, y + h);
  drawDashedLine(x + w, y + h, x, y + h);
  drawDashedLine(x, y + h, x, y);
}

// ── Draw helper ───────────────────────────────────────────────────────────────

/**
 * Draw the visual (non-inflated) hull by transforming local vertices through
 * the body's current position and angle. This keeps the rendered polygon at the
 * original model size while the physics body carries the 1 mm gap margin.
 */
function drawBody(
  gfx: PIXI.Graphics,
  body: Matter.Body,
  visualLocalVerts: Vec2[],
  color: number,
  yOffset = 0,
) {
  gfx.clear();
  if (!visualLocalVerts.length) return;

  const cos = Math.cos(body.angle);
  const sin = Math.sin(body.angle);
  const { x: px, y: py } = body.position;

  gfx.beginFill(color, 0.45);
  gfx.lineStyle(1.5, color, 0.9);
  const v0 = visualLocalVerts[0];
  gfx.moveTo(v0.x * cos - v0.y * sin + px, v0.x * sin + v0.y * cos + py + yOffset);
  for (let i = 1; i < visualLocalVerts.length; i++) {
    const v = visualLocalVerts[i];
    gfx.lineTo(v.x * cos - v.y * sin + px, v.x * sin + v.y * cos + py + yOffset);
  }
  gfx.closePath();
  gfx.endFill();

  if (DEBUG_PHYSICS_WIREFRAME) {
    gfx.lineStyle(1, 0xff0000, 0.5);
    gfx.moveTo(body.vertices[0].x, body.vertices[0].y + yOffset);
    for (let i = 1; i < body.vertices.length; i++)
      gfx.lineTo(body.vertices[i].x, body.vertices[i].y + yOffset);
    gfx.closePath();
  }
}

function getIncrementalSpawnX(
  spawnOrder: SpawnType,
  model: Model,
  models: Model[],
  items: Record<string, number>,
  entries: Entry[],
  hullMode: HullMode,
): number {
  const currentMetrics = getModelFootprintMetrics(model, hullMode);
  if (spawnOrder !== 'largestFirstFillGaps') {
    return clampSpawnX(getRandomSpawnX(), currentMetrics.boundingWidthPx);
  }

  const activeModels = models
    .filter((candidate) => (items[candidate.id] ?? 0) > 0)
    .map((candidate) => ({ candidate, metrics: getModelFootprintMetrics(candidate, hullMode) }))
    .sort((a, b) => compareBySizeDesc(a.metrics, b.metrics) || a.candidate.name.localeCompare(b.candidate.name));

  const largest = activeModels[0];
  if (!largest) {
    return clampSpawnX(getRandomSpawnX(), currentMetrics.boundingWidthPx);
  }

  if (compareBySizeDesc(currentMetrics, largest.metrics) <= 0 && largest.candidate.id !== model.id) {
    const anchorEntry = entries.find((entry) => entry.modelId === largest.candidate.id);
    if (anchorEntry) {
      return clampSpawnX(anchorEntry.body.position.x, currentMetrics.boundingWidthPx);
    }
  }

  return clampSpawnX(getRandomSpawnX(), currentMetrics.boundingWidthPx);
}

// ── Component ─────────────────────────────────────────────────────────────────

export default function PrintingListCanvas({
  models,
  items,
  spawnOrder,
  hullMode,
  onSpawnOrderChange,
  onHullModeChange,
  onPausedChange,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const itemsKey = useMemo(() => JSON.stringify(items), [items]);
  const [resetCount, setResetCount] = useState(0);
  const [pauseOnDrag, setPauseOnDrag] = useState(
    () => localStorage.getItem(PAUSE_ON_DRAG_LOCALSTORAGE_KEY) === 'true',
  );
  const pauseOnDragRef = useRef(pauseOnDrag);
  useEffect(() => {
    pauseOnDragRef.current = pauseOnDrag;
  }, [pauseOnDrag]);

  const [isPaused, setIsPaused] = useState(false);

  // Refs that let the incremental-update effect reach into the running simulation
  const appRef = useRef<PIXI.Application | null>(null);
  const engineRef = useRef<Matter.Engine | null>(null);
  const entriesRef = useRef<Entry[]>([]);
  const dynamicBodiesRef = useRef<Matter.Body[]>([]);
  const modelColorRef = useRef<Map<string, number>>(new Map());
  const pausedRef = useRef(false);
  const prevItemsRef = useRef<Record<string, number>>({});
  // Lets the effect's ticker/handlers sync paused state back to React without stale closures
  const notifyPausedRef = useRef<((v: boolean) => void) | null>(null);
  notifyPausedRef.current = (v) => {
    setIsPaused(v);
    onPausedChange?.(v);
  };
  const saveLayoutRef = useRef<(() => void) | null>(null);
  // Always-current mirrors updated every render
  const itemsKeyRef = useRef(itemsKey);
  itemsKeyRef.current = itemsKey;
  const modelsRef = useRef(models);
  modelsRef.current = models;
  const hullModeRef = useRef<HullMode>(hullMode);
  hullModeRef.current = hullMode;

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    // ── Pixi application ───────────────────────────────────────────────────
    const app = new PIXI.Application({
      width: CANVAS_WIDTH_PX,
      height: VIEW_HEIGHT_PX,
      backgroundColor: 0x0f172a,
      antialias: true,
      resolution: Math.min(window.devicePixelRatio || 1, 2),
      autoDensity: true,
    });
    appRef.current = app;
    container.appendChild(app.view as HTMLCanvasElement);
    app.stage.eventMode = 'static';
    app.stage.hitArea = new PIXI.Rectangle(0, 0, CANVAS_WIDTH_PX, VIEW_HEIGHT_PX);

    // ── Matter.js engine ───────────────────────────────────────────────────
    const engine = Matter.Engine.create({ gravity: { x: 0, y: 1 } });
    engineRef.current = engine;

    const ground = Matter.Bodies.rectangle(
      CANVAS_WIDTH_PX / 2,
      CANVAS_HEIGHT_PX + WALL_THICKNESS / 2,
      CANVAS_WIDTH_PX + WALL_THICKNESS * 2,
      WALL_THICKNESS,
      { isStatic: true },
    );
    const wallLeft = Matter.Bodies.rectangle(
      -WALL_THICKNESS / 2,
      CANVAS_HEIGHT_PX / 2,
      WALL_THICKNESS,
      CANVAS_HEIGHT_PX + WALL_THICKNESS * 2,
      { isStatic: true },
    );
    const wallRight = Matter.Bodies.rectangle(
      CANVAS_WIDTH_PX + WALL_THICKNESS / 2,
      CANVAS_HEIGHT_PX / 2,
      WALL_THICKNESS,
      CANVAS_HEIGHT_PX + WALL_THICKNESS * 2,
      { isStatic: true },
    );
    Matter.Composite.add(engine.world, [ground, wallLeft, wallRight]);

    // ── Background ─────────────────────────────────────────────────────────
    const GRID_PX = 10 * PX_PER_MM;
    const gridGfx = new PIXI.Graphics();
    gridGfx.lineStyle(0.5, 0x334155, 0.6);
    for (let x = GRID_PX; x < CANVAS_WIDTH_PX; x += GRID_PX)
      gridGfx.moveTo(x, VIEW_TOP_MARGIN_PX).lineTo(x, VIEW_TOP_MARGIN_PX + CANVAS_HEIGHT_PX);
    for (let y = GRID_PX; y < CANVAS_HEIGHT_PX; y += GRID_PX)
      gridGfx.moveTo(0, VIEW_TOP_MARGIN_PX + y).lineTo(CANVAS_WIDTH_PX, VIEW_TOP_MARGIN_PX + y);
    app.stage.addChild(gridGfx);

    const borderGfx = new PIXI.Graphics();
    borderGfx.lineStyle(2, 0x64748b, 1);
    borderGfx.drawRect(0, VIEW_TOP_MARGIN_PX, CANVAS_WIDTH_PX, CANVAS_HEIGHT_PX);
    app.stage.addChild(borderGfx);

    const outOfBoundsBorderGfx = new PIXI.Graphics();
    outOfBoundsBorderGfx.lineStyle(5, 0xef4444, 1);
    drawDottedRect(
      outOfBoundsBorderGfx,
      8,
      VIEW_TOP_MARGIN_PX + 8,
      CANVAS_WIDTH_PX - 16,
      CANVAS_HEIGHT_PX - 16,
      12,
      8,
    );
    outOfBoundsBorderGfx.visible = false;
    app.stage.addChild(outOfBoundsBorderGfx);

    // ── Saved layout ───────────────────────────────────────────────────────
    let savedLayout: SavedLayout | null = null;
    try {
      const raw = localStorage.getItem(LAYOUT_LOCALSTORAGE_KEY);
      if (raw) {
        const parsed: SavedLayout = JSON.parse(raw);
        if (parsed.itemsKey === itemsKey) savedLayout = parsed;
      }
    } catch {
      /* ignore */
    }

    // ── Build bodies + graphics ────────────────────────────────────────────
    const entries: Entry[] = [];
    entriesRef.current = entries;
    const dynamicBodies: Matter.Body[] = [];
    dynamicBodiesRef.current = dynamicBodies;

    const modelColor = new Map<string, number>();
    modelColorRef.current = modelColor;
    models.forEach((m, i) => modelColor.set(m.id, PALETTE[i % PALETTE.length]));

    let spawnY = -40; // bodies spawn above the canvas and fall in

    const spawnPlan = buildSpawnPlan(models, items, spawnOrder, hullMode);

    for (const { model, inst, spawnX, metrics } of spawnPlan) {
      const color = modelColor.get(model.id) ?? PALETTE[0];
      const localVerts = metrics.localVerts;

      // Create physics body (inflated) + keep original verts for rendering
      let body: Matter.Body;
      let visualLocalVerts: Vec2[];
      if (localVerts && localVerts.length >= 3) {
        try {
          body = makePolygonBody(spawnX, spawnY, inflateVerts(localVerts, BODY_MARGIN_PX));
          visualLocalVerts = localVerts;
        } catch {
          ({ body, visualLocalVerts } = makeRectBody(model, spawnX, spawnY));
        }
      } else {
        ({ body, visualLocalVerts } = makeRectBody(model, spawnX, spawnY));
      }

      // If there's a saved position for this instance, move the body there
      const saved = savedLayout?.positions.find(
        (p) => p.modelId === model.id && p.instanceIndex === inst,
      );
      if (saved) {
        Matter.Body.setPosition(body, { x: saved.xMm * PX_PER_MM, y: saved.yMm * PX_PER_MM });
        Matter.Body.setAngle(body, saved.angle);
        Matter.Body.setVelocity(body, { x: 0, y: 0 });
        Matter.Body.setAngularVelocity(body, 0);
      }

      Matter.Composite.add(engine.world, body);
      dynamicBodies.push(body);

      // Spread spawn points above canvas so bodies don't all overlap
      spawnY -= 50 + Math.max(metrics.boundingHeightPx, 60);

      // Pixi graphics (drawn in world-space each frame)
      const gfx = new PIXI.Graphics();
      app.stage.addChild(gfx);

      // Label
      const labelStr = model.name.length > 14 ? model.name.slice(0, 12) + '…' : model.name;
      const label = new PIXI.Text(labelStr, {
        fontFamily: 'system-ui, -apple-system, sans-serif',
        fontSize: 8,
        fill: 0xffffff,
        align: 'center',
      });
      label.anchor.set(0.5);
      app.stage.addChild(label);

      entries.push({
        body,
        gfx,
        label,
        modelId: model.id,
        instanceIndex: inst,
        color,
        visualLocalVerts,
      });
    }

    // ── Simulation state ───────────────────────────────────────────────────
    // Start paused if positions were restored from storage; user clicks to resume.
    pausedRef.current = savedLayout !== null;
    notifyPausedRef.current?.(pausedRef.current);
    // Record which items are now in the simulation (for incremental updates)
    prevItemsRef.current = { ...items };
    let settleFrames = 0;
    let drag: { body: Matter.Body; ox: number; oy: number } | null = null;

    // ── Render helper ──────────────────────────────────────────────────────
    function renderEntries(overlapping: Set<number>) {
      let hasOutOfBounds = false;
      for (const { body, gfx, label, color, visualLocalVerts } of entries) {
        const renderColor = overlapping.has(body.id) ? color : darkenColor(color, 0.45);
        drawBody(gfx, body, visualLocalVerts, renderColor, VIEW_TOP_MARGIN_PX);
        label.position.set(body.position.x, body.position.y + VIEW_TOP_MARGIN_PX);
        hasOutOfBounds ||= isOutOfBounds(body, visualLocalVerts);
      }
      outOfBoundsBorderGfx.visible = hasOutOfBounds;
    }

    // ── Layout persistence ─────────────────────────────────────────────────
    function saveLayout() {
      const layout: SavedLayout = {
        itemsKey: itemsKeyRef.current,
        positions: entries.map((e) => ({
          modelId: e.modelId,
          instanceIndex: e.instanceIndex,
          xMm: e.body.position.x / PX_PER_MM,
          yMm: e.body.position.y / PX_PER_MM,
          angle: e.body.angle,
        })),
      };
      localStorage.setItem(LAYOUT_LOCALSTORAGE_KEY, JSON.stringify(layout));
    }
    saveLayoutRef.current = saveLayout;

    // ── Drag interaction ───────────────────────────────────────────────────
    app.stage.on('pointerdown', (e: PIXI.FederatedPointerEvent) => {
      // Resume simulation on any click when paused
      if (pausedRef.current) {
        pausedRef.current = false;
        notifyPausedRef.current?.(false);
        settleFrames = 0;
      }

      const { x, y } = e.global;
      const worldY = y - VIEW_TOP_MARGIN_PX;
      const hits = Matter.Query.point(dynamicBodies, { x, y: worldY });
      if (hits.length > 0) {
        const hit = hits[0];
        drag = { body: hit, ox: x - hit.position.x, oy: worldY - hit.position.y };
        Matter.Body.setStatic(hit, true);
        if (pauseOnDragRef.current) {
          pausedRef.current = true;
          notifyPausedRef.current?.(true);
        }
      }
    });

    app.stage.on('pointermove', (e: PIXI.FederatedPointerEvent) => {
      if (!drag) return;
      const { x, y } = e.global;
      const worldY = y - VIEW_TOP_MARGIN_PX;
      Matter.Body.setPosition(drag.body, { x: x - drag.ox, y: worldY - drag.oy });
    });

    const endDrag = () => {
      if (!drag) return;
      Matter.Body.setStatic(drag.body, false);
      Matter.Body.setVelocity(drag.body, { x: 0, y: 0 });
      Matter.Body.setAngularVelocity(drag.body, 0);
      drag = null;
      pausedRef.current = false;
      notifyPausedRef.current?.(false);
      settleFrames = 0;
    };
    app.stage.on('pointerup', endDrag);
    app.stage.on('pointerupoutside', endDrag);

    const canvas = app.view as HTMLCanvasElement;
    const onWheel = (e: WheelEvent) => {
      if (!drag) return;
      e.preventDefault();
      Matter.Body.setAngle(drag.body, drag.body.angle + e.deltaY * 0.003);
    };
    canvas.addEventListener('wheel', onWheel, { passive: false });

    // ── Ticker ─────────────────────────────────────────────────────────────
    app.ticker.add(() => {
      if (!pausedRef.current) {
        // Cap delta to avoid physics explosion after tab becomes active again
        const deltaMs = Math.min(app.ticker.deltaMS, 50);
        Matter.Engine.update(engine, deltaMs);
      }

      renderEntries(computeOverlapping(entries));

      // Settling detection (skip when dragging or already paused)
      if (!pausedRef.current && !drag && dynamicBodies.length > 0) {
        const allSlow = dynamicBodies.every(
          (b) => b.speed < SPEED_THRESH && Math.abs(b.angularSpeed) < ANGULAR_THRESH,
        );
        if (allSlow) {
          settleFrames++;
          if (settleFrames >= SETTLE_FRAMES) {
            pausedRef.current = true;
            notifyPausedRef.current?.(true);
            saveLayout();
          }
        } else {
          settleFrames = 0;
        }
      }
    });

    // ── Cleanup ────────────────────────────────────────────────────────────
    return () => {
      canvas.removeEventListener('wheel', onWheel);
      appRef.current = null;
      engineRef.current = null;
      entriesRef.current = [];
      dynamicBodiesRef.current = [];
      modelColorRef.current = new Map();
      saveLayoutRef.current = null;
      app.destroy(true, { children: true, texture: true, baseTexture: true });
      Matter.Engine.clear(engine);
    };
  }, [spawnOrder, hullMode, resetCount]);

  // ── Incremental effect: add/remove bodies when item counts change ──────────
  useEffect(() => {
    const app = appRef.current;
    const engine = engineRef.current;
    if (!app || !engine) return; // simulation not yet initialised

    const prevItems = prevItemsRef.current;
    const currItems = items;
    const currentModels = modelsRef.current;

    const allModelIds = new Set([...Object.keys(prevItems), ...Object.keys(currItems)]);

    for (const modelId of allModelIds) {
      const prevQty = prevItems[modelId] ?? 0;
      const currQty = currItems[modelId] ?? 0;
      if (prevQty === currQty) continue;

      const model = currentModels.find((m) => m.id === modelId);
      if (!model) continue;

      if (currQty > prevQty) {
        // ── Add new instances (from prevQty up to currQty-1) ───────────────
        // Assign a palette colour if this model hasn't appeared before
        if (!modelColorRef.current.has(modelId)) {
          modelColorRef.current.set(modelId, PALETTE[modelColorRef.current.size % PALETTE.length]);
        }
        let spawnY = -80;
        for (let inst = prevQty; inst < currQty; inst++) {
          const color = modelColorRef.current.get(modelId)!;
          const metrics = getModelFootprintMetrics(model, hullModeRef.current);
          const localVerts = metrics.localVerts;
          const spawnX = getIncrementalSpawnX(
            spawnOrder,
            model,
            currentModels,
            currItems,
            entriesRef.current,
            hullModeRef.current,
          );

          let body: Matter.Body;
          let visualLocalVerts: Vec2[];
          if (localVerts && localVerts.length >= 3) {
            try {
              body = makePolygonBody(spawnX, spawnY, inflateVerts(localVerts, BODY_MARGIN_PX));
              visualLocalVerts = localVerts;
            } catch {
              ({ body, visualLocalVerts } = makeRectBody(model, spawnX, spawnY));
            }
          } else {
            ({ body, visualLocalVerts } = makeRectBody(model, spawnX, spawnY));
          }

          spawnY -= 50 + Math.max(metrics.boundingHeightPx, 60);

          Matter.Composite.add(engine.world, body);
          dynamicBodiesRef.current.push(body);

          const gfx = new PIXI.Graphics();
          app.stage.addChild(gfx);

          const label = new PIXI.Text(
            model.name.length > 14 ? model.name.slice(0, 12) + '…' : model.name,
            {
              fontFamily: 'system-ui, -apple-system, sans-serif',
              fontSize: 8,
              fill: 0xffffff,
              align: 'center',
            },
          );
          label.anchor.set(0.5);
          app.stage.addChild(label);

          entriesRef.current.push({
            body,
            gfx,
            label,
            modelId,
            instanceIndex: inst,
            color,
            visualLocalVerts,
          });
        }

        pausedRef.current = false; // let new bodies fall in
      } else {
        // Remove the models at the top of the build plate first
        for (let i = prevQty; i > currQty; i--) {
          let minY = Infinity;
          let toRemove: Entry | null = null;
          for (const e of entriesRef.current) {
            if (e.modelId === modelId && e.body.position.y < minY) {
              minY = e.body.position.y;
              toRemove = e;
            }
          }
          if (!toRemove) break;

          Matter.Composite.remove(engine.world, toRemove.body);
          const dynIdx = dynamicBodiesRef.current.indexOf(toRemove.body);
          if (dynIdx >= 0) dynamicBodiesRef.current.splice(dynIdx, 1);

          app.stage.removeChild(toRemove.gfx);
          toRemove.gfx.destroy();
          app.stage.removeChild(toRemove.label);
          toRemove.label.destroy();

          const eIdx = entriesRef.current.indexOf(toRemove);
          if (eIdx >= 0) entriesRef.current.splice(eIdx, 1);
        }
      }
    }

    prevItemsRef.current = { ...currItems };
  }, [itemsKey, spawnOrder]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <Stack direction="column" spacing={1}>
      <Stack direction="row" spacing={1} alignContent={'center'} alignItems="center">
        <Typography variant="body2" color="text.secondary">
          Printer: Uniformation GK2
          <br />
          Print area: {CANVAS_WIDTH_MM} × {CANVAS_HEIGHT_MM} mm
        </Typography>
        <FormControl size="small">
          <InputLabel id="spawn-order-label">Spawn order</InputLabel>
          <Select
            labelId="spawn-order-label"
            label="Spawn order"
            value={spawnOrder}
            onChange={(e) => {
              const next = e.target.value as SpawnType;
              localStorage.removeItem(LAYOUT_LOCALSTORAGE_KEY);
              onSpawnOrderChange?.(next);
            }}
          >
            <MenuItem value="grouped">Grouped</MenuItem>
            <MenuItem value="random">Random</MenuItem>
            <MenuItem value="largestFirstFillGaps">Largest first, fill gaps</MenuItem>
          </Select>
        </FormControl>
        <FormControl size="small">
          <InputLabel id="hull-mode-label">Hull mode</InputLabel>
          <Select
            labelId="hull-mode-label"
            label="Hull mode"
            value={hullMode}
            onChange={(e) => {
              const next = e.target.value as HullMode;
              localStorage.removeItem(LAYOUT_LOCALSTORAGE_KEY);
              onHullModeChange?.(next);
            }}
          >
            <MenuItem value="convex">Convex hull</MenuItem>
            <MenuItem value="sansRaft">Sans raft hull</MenuItem>
          </Select>
        </FormControl>
        <Button
          size="large"
          variant="outlined"
          onClick={() => {
            pausedRef.current = true;
            notifyPausedRef.current?.(true);
            saveLayoutRef.current?.();
          }}
          disabled={isPaused}
        >
          Save
        </Button>
        <Button
          size="large"
          variant="outlined"
          onClick={() => {
            localStorage.removeItem(LAYOUT_LOCALSTORAGE_KEY);
            setResetCount((c) => c + 1);
          }}
        >
          Reset
        </Button>
        <FormGroup>
          <FormControlLabel
            label="Pause on drag"
            control={
              <Checkbox
                checked={pauseOnDrag}
                onChange={() => {
                  const next = !pauseOnDrag;
                  localStorage.setItem(PAUSE_ON_DRAG_LOCALSTORAGE_KEY, String(next));
                  setPauseOnDrag(next);
                }}
              />
            }
          />
        </FormGroup>
      </Stack>
      <div
        ref={containerRef}
        style={{
          width: CANVAS_WIDTH_PX,
          height: VIEW_HEIGHT_PX,
          borderRadius: 0,
          overflow: 'hidden',
          boxShadow: '0 4px 24px rgba(0,0,0,0.4)',
          cursor: 'default',
        }}
      />
    </Stack>
  );
}
