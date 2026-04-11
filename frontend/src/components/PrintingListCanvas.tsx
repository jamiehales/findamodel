import { useEffect, useMemo, useRef, useState } from 'react';
import * as PIXI from 'pixi.js';
import { World, Vec2, Polygon, Box, Settings } from 'planck';
import type { Body } from 'planck';
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

// Allow complex convex hulls with many vertices
Settings.maxPolygonVertices = 64;

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
const SPEED_THRESH = 0.005;
const ANGULAR_THRESH = 0.003;
const SETTLE_FRAMES = 90; // ~1.5 s at 60 fps
export const LAYOUT_LOCALSTORAGE_KEY = 'findamodel.printingListLayout';
const PAUSE_ON_DRAG_LOCALSTORAGE_KEY = 'findamodel.printingListPauseOnDrag';
const DEBUG_PHYSICS_WIREFRAME = false;
const PHYSICS_BORDER_PADDING_PX = 5;

// Physics body inflation — when two bodies touch they have BODY_GAP_MM visual gap.
const BODY_GAP_MM = 2;
const BODY_MARGIN_PX = (BODY_GAP_MM / 2) * PX_PER_MM;

// ── Physics scale ─────────────────────────────────────────────────────────────
// planck.js (Box2D) is tuned for objects 0.1–10 m.  1 physics unit = 100 px.
const PHYSICS_SCALE = 100;
const toPhysics = (px: number) => px / PHYSICS_SCALE;
const toPixels = (p: number) => p * PHYSICS_SCALE;
const CANVAS_WIDTH_PHYS = toPhysics(CANVAS_WIDTH_PX);
const CANVAS_HEIGHT_PHYS = toPhysics(CANVAS_HEIGHT_PX);

// ── Collision filtering ───────────────────────────────────────────────────────
// Three categories allow per-fixture routing:
//   WALL   fixtures only touch BORDER fixtures  (keeps models on the plate)
//   OBJECT fixtures only touch other OBJECT fixtures (object-object spacing)
//   BORDER fixtures only touch WALL   fixtures  (plate edge containment)
const CAT_WALL = 0x0001;
const CAT_OBJECT = 0x0002;
const CAT_BORDER = 0x0004;

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

interface Vec2Like {
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
  body: Body;
  gfx: PIXI.Graphics;
  label: PIXI.Text;
  modelId: string;
  instanceIndex: number;
  color: number;
  /** Original (non-inflated) spawn hull vertices in body-local pixel space, for rendering. */
  visualLocalVerts: Vec2Like[];
  /** Border hull vertices in body-local pixel space (always with-raft), for bounds checking. */
  borderLocalVerts: Vec2Like[];
}

interface ModelFootprintMetrics {
  bodyHullJson: string | null;
  borderHullJson: string | null;
  localVerts: Vec2Like[] | null;
  borderLocalVerts: Vec2Like[] | null;
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
function parseHullLocalPx(hullJson: string | null): Vec2Like[] | null {
  if (!hullJson) return null;
  try {
    const raw: [number, number][] = JSON.parse(hullJson);
    if (raw.length < 3) return null;

    // Hull coords are [x, z] in model space (assumed mm for 3-D printing STLs).
    // Map z → canvas y.
    const pts = raw.map(([x, z]): Vec2Like => ({ x: x * PX_PER_MM, y: z * PX_PER_MM }));

    // Centre at the polygon area centroid (shoelace formula)
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
  return hullMode === 'sansRaft'
    ? (model.convexSansRaftHull ?? model.convexHull)
    : model.convexHull;
}

/** Border hull always uses the full convex hull (with raft). */
function getBorderHullJson(model: Model): string | null {
  return model.convexHull;
}

function getFillEstimateHullJson(model: Model, hullMode: HullMode): string | null {
  return model.concaveHull ?? getSpawnHullJson(model, hullMode);
}

// ── Geometry helpers ──────────────────────────────────────────────────────────

function polygonArea(verts: Vec2Like[]): number {
  if (verts.length < 3) return 0;

  let area = 0;
  for (let i = 0; i < verts.length; i++) {
    const a = verts[i];
    const b = verts[(i + 1) % verts.length];
    area += a.x * b.y - b.x * a.y;
  }

  return Math.abs(area) / 2;
}

function getBounds(verts: Vec2Like[]): { minX: number; maxX: number; minY: number; maxY: number } {
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

function getRectLocalVerts(model: Model): Vec2Like[] {
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
  const borderHullJson = getBorderHullJson(model);
  const localVerts = parseHullLocalPx(bodyHullJson);
  const borderLocalVerts = parseHullLocalPx(borderHullJson);
  const rectLocalVerts = getRectLocalVerts(model);
  const boundingVerts = localVerts && localVerts.length >= 3 ? localVerts : rectLocalVerts;
  const fillVerts = parseHullLocalPx(getFillEstimateHullJson(model, hullMode)) ?? boundingVerts;
  const bounds = getBounds(boundingVerts);
  const boundingWidthPx = Math.max(bounds.maxX - bounds.minX, 16);
  const boundingHeightPx = Math.max(bounds.maxY - bounds.minY, 16);

  return {
    bodyHullJson,
    borderHullJson,
    localVerts,
    borderLocalVerts,
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

    let remainingGapArea = Math.max(
      0,
      largest.metrics.boundingAreaPx2 - largest.metrics.footprintAreaPx2,
    );
    if (remainingGapArea <= 0) continue;

    const fillers = remaining
      .filter(
        (candidate) =>
          candidate.metrics.footprintAreaPx2 > 0 &&
          candidate.metrics.boundingAreaPx2 < largest.metrics.boundingAreaPx2,
      )
      .sort(
        (a, b) =>
          a.metrics.footprintAreaPx2 - b.metrics.footprintAreaPx2 ||
          a.metrics.boundingAreaPx2 - b.metrics.boundingAreaPx2 ||
          a.model.name.localeCompare(b.model.name) ||
          a.inst - b.inst,
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
function inflateVerts(verts: Vec2Like[], amount: number): Vec2Like[] {
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

/**
 * Ensure vertices are in the winding order that planck.js (Box2D) expects for
 * valid convex polygons.  In our Y-down screen coordinate system, planck needs
 * the signed area to be positive (screen-CW = math-CCW).
 */
function ensurePlanckWinding(verts: Vec2Like[]): Vec2Like[] {
  let signedArea2 = 0;
  for (let i = 0; i < verts.length; i++) {
    const a = verts[i];
    const b = verts[(i + 1) % verts.length];
    signedArea2 += a.x * b.y - b.x * a.y;
  }
  if (signedArea2 < 0) return [...verts].reverse();
  return verts;
}

/** Convert local pixel-space vertices to planck Vec2 in physics units. */
function toPhysicsVerts(pxVerts: Vec2Like[]): Vec2Like[] {
  return ensurePlanckWinding(pxVerts).map((v) => ({ x: toPhysics(v.x), y: toPhysics(v.y) }));
}

// ── Body factory helpers ──────────────────────────────────────────────────────

const FIXTURE_OPT_OBJECT = {
  density: 1.0,
  restitution: 0.05,
  friction: 0.5,
  filterCategoryBits: CAT_OBJECT,
  filterMaskBits: CAT_OBJECT,
};

const FIXTURE_OPT_BORDER = {
  density: 0, // border fixture doesn't affect mass
  restitution: 0.05,
  friction: 0.5,
  filterCategoryBits: CAT_BORDER,
  filterMaskBits: CAT_WALL,
};

const BODY_DEF_DYNAMIC = {
  type: 'dynamic' as const,
  linearDamping: 1.5,
  angularDamping: 1.5,
};

/**
 * Create a dynamic body with two fixtures:
 *  1. Object-object collision fixture (spawn hull, inflated for gap)
 *  2. Border collision fixture (always with-raft hull, not inflated)
 *
 * Returns the body and the visual/border local verts in pixel space (for rendering).
 */
function createModelBody(
  world: InstanceType<typeof World>,
  cxPx: number,
  cyPx: number,
  spawnLocalVerts: Vec2Like[] | null,
  borderLocalVerts: Vec2Like[] | null,
  model: Model,
): { body: Body; visualLocalVerts: Vec2Like[]; borderLocalVerts: Vec2Like[] } {
  const body = world.createDynamicBody({
    ...BODY_DEF_DYNAMIC,
    position: Vec2(toPhysics(cxPx), toPhysics(cyPx)),
  });

  let visualLocal: Vec2Like[];
  let borderLocal: Vec2Like[];

  // ── Object-object fixture (spawn hull) ──────────────────────────────────
  if (spawnLocalVerts && spawnLocalVerts.length >= 3) {
    try {
      const inflated = inflateVerts(spawnLocalVerts, BODY_MARGIN_PX);
      body.createFixture(new Polygon(toPhysicsVerts(inflated)), FIXTURE_OPT_OBJECT);
      visualLocal = spawnLocalVerts;
    } catch {
      // Hull rejected by planck – fall back to rectangle
      const rectVerts = getRectLocalVerts(model);
      const bounds = getBounds(rectVerts);
      const hw = toPhysics((bounds.maxX - bounds.minX) / 2 + BODY_MARGIN_PX);
      const hh = toPhysics((bounds.maxY - bounds.minY) / 2 + BODY_MARGIN_PX);
      body.createFixture(new Box(hw, hh), FIXTURE_OPT_OBJECT);
      visualLocal = rectVerts;
    }
  } else {
    const rectVerts = getRectLocalVerts(model);
    const bounds = getBounds(rectVerts);
    const hw = toPhysics((bounds.maxX - bounds.minX) / 2 + BODY_MARGIN_PX);
    const hh = toPhysics((bounds.maxY - bounds.minY) / 2 + BODY_MARGIN_PX);
    body.createFixture(new Box(hw, hh), FIXTURE_OPT_OBJECT);
    visualLocal = rectVerts;
  }

  // ── Border fixture (always with-raft hull) ──────────────────────────────
  if (borderLocalVerts && borderLocalVerts.length >= 3) {
    try {
      body.createFixture(new Polygon(toPhysicsVerts(borderLocalVerts)), FIXTURE_OPT_BORDER);
      borderLocal = borderLocalVerts;
    } catch {
      // Fall back to rectangle for border too
      const rectVerts = getRectLocalVerts(model);
      const bounds = getBounds(rectVerts);
      const hw = toPhysics((bounds.maxX - bounds.minX) / 2);
      const hh = toPhysics((bounds.maxY - bounds.minY) / 2);
      body.createFixture(new Box(hw, hh), FIXTURE_OPT_BORDER);
      borderLocal = rectVerts;
    }
  } else {
    // No border hull – use same rect as object fixture but without inflation
    const rectVerts = getRectLocalVerts(model);
    const bounds = getBounds(rectVerts);
    const hw = toPhysics((bounds.maxX - bounds.minX) / 2);
    const hh = toPhysics((bounds.maxY - bounds.minY) / 2);
    body.createFixture(new Box(hw, hh), FIXTURE_OPT_BORDER);
    borderLocal = rectVerts;
  }

  return { body, visualLocalVerts: visualLocal, borderLocalVerts: borderLocal };
}

// ── Color helpers ─────────────────────────────────────────────────────────────

function darkenColor(color: number, factor: number): number {
  const r = Math.round(((color >> 16) & 0xff) * factor);
  const g = Math.round(((color >> 8) & 0xff) * factor);
  const b = Math.round((color & 0xff) * factor);
  return (r << 16) | (g << 8) | b;
}

// ── Overlap helpers ───────────────────────────────────────────────────────────

/** Transform local-space pixel vertices to world pixel space using a body's pose. */
function toWorldVerts(body: Body, localVerts: Vec2Like[]): Vec2Like[] {
  const pos = body.getPosition();
  const px = toPixels(pos.x);
  const py = toPixels(pos.y);
  const angle = body.getAngle();
  const cos = Math.cos(angle);
  const sin = Math.sin(angle);
  return localVerts.map((v) => ({
    x: v.x * cos - v.y * sin + px,
    y: v.x * sin + v.y * cos + py,
  }));
}

/**
 * Separating Axis Theorem overlap test for two convex polygons.
 * Returns true if they overlap (no separating axis found).
 */
function satOverlap(a: Vec2Like[], b: Vec2Like[]): boolean {
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

/** Returns the set of bodies whose visual hulls intersect any other entry. */
function computeOverlapping(entries: Entry[]): Set<Body> {
  const overlapping = new Set<Body>();
  const worldVerts = entries.map((e) => toWorldVerts(e.body, e.visualLocalVerts));
  for (let i = 0; i < entries.length; i++) {
    for (let j = i + 1; j < entries.length; j++) {
      if (satOverlap(worldVerts[i], worldVerts[j])) {
        overlapping.add(entries[i].body);
        overlapping.add(entries[j].body);
      }
    }
  }
  return overlapping;
}

/** Visual bounds check: true when any border hull vertex is outside plate rectangle. */
function isOutOfBounds(body: Body, borderLocalVerts: Vec2Like[]): boolean {
  if (!borderLocalVerts.length) return false;
  return borderLocalVerts.some((v) => {
    const w = body.getWorldPoint(Vec2(toPhysics(v.x), toPhysics(v.y)));
    return w.x < 0 || w.x > CANVAS_WIDTH_PHYS || w.y < 0 || w.y > CANVAS_HEIGHT_PHYS;
  });
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
 * original model size while the physics body carries the gap margin.
 */
function drawBody(
  gfx: PIXI.Graphics,
  body: Body,
  visualLocalVerts: Vec2Like[],
  borderLocalVerts: Vec2Like[],
  color: number,
  highlightOutOfBounds = false,
  yOffset = 0,
) {
  gfx.clear();
  if (!visualLocalVerts.length) return;

  const pos = body.getPosition();
  const px = toPixels(pos.x);
  const py = toPixels(pos.y);
  const angle = body.getAngle();
  const cos = Math.cos(angle);
  const sin = Math.sin(angle);

  gfx.beginFill(color, highlightOutOfBounds ? 0.62 : 0.45);
  gfx.lineStyle(highlightOutOfBounds ? 3 : 1.5, highlightOutOfBounds ? 0xef4444 : color, 0.95);
  const v0 = visualLocalVerts[0];
  gfx.moveTo(v0.x * cos - v0.y * sin + px, v0.x * sin + v0.y * cos + py + yOffset);
  for (let i = 1; i < visualLocalVerts.length; i++) {
    const v = visualLocalVerts[i];
    gfx.lineTo(v.x * cos - v.y * sin + px, v.x * sin + v.y * cos + py + yOffset);
  }
  gfx.closePath();
  gfx.endFill();

  // When out of bounds, draw a red outline using the with-raft border hull.
  if (highlightOutOfBounds && borderLocalVerts.length) {
    gfx.lineStyle(3, 0xef4444, 0.98);
    const b0 = borderLocalVerts[0];
    gfx.moveTo(b0.x * cos - b0.y * sin + px, b0.x * sin + b0.y * cos + py + yOffset);
    for (let i = 1; i < borderLocalVerts.length; i++) {
      const v = borderLocalVerts[i];
      gfx.lineTo(v.x * cos - v.y * sin + px, v.x * sin + v.y * cos + py + yOffset);
    }
    gfx.closePath();
  }

  if (DEBUG_PHYSICS_WIREFRAME) {
    // Draw each fixture wireframe in a different colour
    let fixture = body.getFixtureList();
    while (fixture) {
      const shape = fixture.getShape();
      if (shape.getType() === 'polygon') {
        const poly = shape as Polygon;
        const vCount = poly.m_count;
        if (vCount > 0) {
          gfx.lineStyle(
            1,
            fixture.getFilterCategoryBits() === CAT_OBJECT ? 0xff0000 : 0x00ff00,
            0.5,
          );
          const wp0 = body.getWorldPoint(poly.m_vertices[0]);
          gfx.moveTo(toPixels(wp0.x), toPixels(wp0.y) + yOffset);
          for (let vi = 1; vi < vCount; vi++) {
            const wp = body.getWorldPoint(poly.m_vertices[vi]);
            gfx.lineTo(toPixels(wp.x), toPixels(wp.y) + yOffset);
          }
          gfx.closePath();
        }
      }
      fixture = fixture.getNext();
    }
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
    .sort(
      (a, b) =>
        compareBySizeDesc(a.metrics, b.metrics) || a.candidate.name.localeCompare(b.candidate.name),
    );

  const largest = activeModels[0];
  if (!largest) {
    return clampSpawnX(getRandomSpawnX(), currentMetrics.boundingWidthPx);
  }

  if (
    compareBySizeDesc(currentMetrics, largest.metrics) <= 0 &&
    largest.candidate.id !== model.id
  ) {
    const anchorEntry = entries.find((entry) => entry.modelId === largest.candidate.id);
    if (anchorEntry) {
      return clampSpawnX(
        toPixels(anchorEntry.body.getPosition().x),
        currentMetrics.boundingWidthPx,
      );
    }
  }

  return clampSpawnX(getRandomSpawnX(), currentMetrics.boundingWidthPx);
}

// ── Point query helper ────────────────────────────────────────────────────────

/** Find the first dynamic body whose fixture contains the given pixel-space point. */
function queryPointPx(world: InstanceType<typeof World>, xPx: number, yPx: number): Body | null {
  const ppx = toPhysics(xPx);
  const ppy = toPhysics(yPx);
  const d = toPhysics(2); // small search radius
  let hitBody: Body | null = null;

  world.queryAABB(
    { lowerBound: Vec2(ppx - d, ppy - d), upperBound: Vec2(ppx + d, ppy + d) },
    (fixture) => {
      if (fixture.getBody().isDynamic() && fixture.testPoint(Vec2(ppx, ppy))) {
        hitBody = fixture.getBody();
        return false; // stop search
      }
      return true; // continue
    },
  );
  return hitBody;
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

  const [hasSavedLayout, setHasSavedLayout] = useState(
    () => localStorage.getItem(LAYOUT_LOCALSTORAGE_KEY) !== null,
  );
  const [isLayoutClean, setIsLayoutClean] = useState(
    () => localStorage.getItem(LAYOUT_LOCALSTORAGE_KEY) !== null,
  );
  const [undoLayoutData, setUndoLayoutData] = useState<string | null>(null);
  const notifySavedLayoutRef = useRef<((v: boolean) => void) | null>(null);
  notifySavedLayoutRef.current = setHasSavedLayout;
  // Drives isLayoutClean + the parent Export-button gate (onPausedChange)
  const notifyLayoutCleanRef = useRef<((v: boolean) => void) | null>(null);
  notifyLayoutCleanRef.current = (v) => {
    setIsLayoutClean(v);
    onPausedChange?.(v);
  };

  // Refs that let the incremental-update effect reach into the running simulation
  const appRef = useRef<PIXI.Application | null>(null);
  const worldRef = useRef<InstanceType<typeof World> | null>(null);
  const entriesRef = useRef<Entry[]>([]);
  const dynamicBodiesRef = useRef<Body[]>([]);
  const modelColorRef = useRef<Map<string, number>>(new Map());
  const pausedRef = useRef(false);
  const prevItemsRef = useRef<Record<string, number>>({});
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

    // ── planck.js world ────────────────────────────────────────────────────
    const world = new World(Vec2(0, 20));
    worldRef.current = world;

    // ── Walls (static, CAT_WALL — only collide with CAT_BORDER fixtures) ──
    const wallFixOpt = {
      filterCategoryBits: CAT_WALL,
      filterMaskBits: CAT_BORDER,
      friction: 0.5,
    };

    const ground = world.createBody({
      type: 'static' as const,
      position: Vec2(
        toPhysics(CANVAS_WIDTH_PX / 2),
        toPhysics(CANVAS_HEIGHT_PX + WALL_THICKNESS / 2 - PHYSICS_BORDER_PADDING_PX),
      ),
    });
    ground.createFixture(
      new Box(toPhysics((CANVAS_WIDTH_PX + WALL_THICKNESS * 2) / 2), toPhysics(WALL_THICKNESS / 2)),
      wallFixOpt,
    );

    const wallLeft = world.createBody({
      type: 'static' as const,
      position: Vec2(toPhysics(-WALL_THICKNESS / 2), toPhysics(CANVAS_HEIGHT_PX / 2)),
    });
    wallLeft.createFixture(
      new Box(
        toPhysics(WALL_THICKNESS / 2),
        toPhysics((CANVAS_HEIGHT_PX + WALL_THICKNESS * 2) / 2),
      ),
      wallFixOpt,
    );

    const wallRight = world.createBody({
      type: 'static' as const,
      position: Vec2(
        toPhysics(CANVAS_WIDTH_PX + WALL_THICKNESS / 2),
        toPhysics(CANVAS_HEIGHT_PX / 2),
      ),
    });
    wallRight.createFixture(
      new Box(
        toPhysics(WALL_THICKNESS / 2),
        toPhysics((CANVAS_HEIGHT_PX + WALL_THICKNESS * 2) / 2),
      ),
      wallFixOpt,
    );

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
    borderGfx.lineStyle({ width: 2, color: 0x64748b, alpha: 1, alignment: 0 });
    borderGfx.drawRect(0, VIEW_TOP_MARGIN_PX, CANVAS_WIDTH_PX, CANVAS_HEIGHT_PX);
    app.stage.addChild(borderGfx);

    const outOfBoundsBorderGfx = new PIXI.Graphics();
    outOfBoundsBorderGfx.lineStyle({ width: 2, color: 0xef4444, alpha: 1, alignment: 0 });
    drawDottedRect(
      outOfBoundsBorderGfx,
      0,
      VIEW_TOP_MARGIN_PX,
      CANVAS_WIDTH_PX,
      CANVAS_HEIGHT_PX,
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
    const dynamicBodies: Body[] = [];
    dynamicBodiesRef.current = dynamicBodies;

    const modelColor = new Map<string, number>();
    modelColorRef.current = modelColor;
    models.forEach((m, i) => modelColor.set(m.id, PALETTE[i % PALETTE.length]));

    let spawnY = -40; // bodies spawn above the canvas and fall in

    const spawnPlan = buildSpawnPlan(models, items, spawnOrder, hullMode);

    for (const { model, inst, spawnX, metrics } of spawnPlan) {
      const color = modelColor.get(model.id) ?? PALETTE[0];

      const { body, visualLocalVerts, borderLocalVerts } = createModelBody(
        world,
        spawnX,
        spawnY,
        metrics.localVerts,
        metrics.borderLocalVerts,
        model,
      );

      // If there's a saved position for this instance, move the body there
      const saved = savedLayout?.positions.find(
        (p) => p.modelId === model.id && p.instanceIndex === inst,
      );
      if (saved) {
        body.setPosition(Vec2(toPhysics(saved.xMm * PX_PER_MM), toPhysics(saved.yMm * PX_PER_MM)));
        body.setAngle(saved.angle);
        body.setLinearVelocity(Vec2(0, 0));
        body.setAngularVelocity(0);
      }

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
        borderLocalVerts,
      });
    }

    // ── Simulation state ───────────────────────────────────────────────────
    // Start paused if positions were restored from storage; user clicks to resume.
    // Only autosave on settle when there was no prior layout — with an existing
    // layout the user must explicitly save after adjusting positions.
    const shouldAutosave = savedLayout === null;
    pausedRef.current = savedLayout !== null;
    notifyLayoutCleanRef.current?.(savedLayout !== null);
    // Record which items are now in the simulation (for incremental updates)
    prevItemsRef.current = { ...items };
    let settleFrames = 0;
    let drag: { body: Body; ox: number; oy: number } | null = null;

    // ── Render helper ──────────────────────────────────────────────────────
    function renderEntries(overlapping: Set<Body>) {
      let hasOutOfBounds = false;
      for (const {
        body,
        gfx,
        label,
        color,
        visualLocalVerts,
        borderLocalVerts: bVerts,
      } of entries) {
        const outOfBounds = isOutOfBounds(body, bVerts);
        const renderColor = overlapping.has(body) ? color : darkenColor(color, 0.45);
        drawBody(gfx, body, visualLocalVerts, bVerts, renderColor, outOfBounds, VIEW_TOP_MARGIN_PX);
        const pos = body.getPosition();
        label.position.set(toPixels(pos.x), toPixels(pos.y) + VIEW_TOP_MARGIN_PX);
        hasOutOfBounds ||= outOfBounds;
      }
      outOfBoundsBorderGfx.visible = hasOutOfBounds;
    }

    // ── Layout persistence ─────────────────────────────────────────────────
    function saveLayout() {
      const layout: SavedLayout = {
        itemsKey: itemsKeyRef.current,
        positions: entries.map((e) => {
          const pos = e.body.getPosition();
          return {
            modelId: e.modelId,
            instanceIndex: e.instanceIndex,
            xMm: toPixels(pos.x) / PX_PER_MM,
            yMm: toPixels(pos.y) / PX_PER_MM,
            angle: e.body.getAngle(),
          };
        }),
      };
      localStorage.setItem(LAYOUT_LOCALSTORAGE_KEY, JSON.stringify(layout));
      notifySavedLayoutRef.current?.(true);
      notifyLayoutCleanRef.current?.(true);
    }
    saveLayoutRef.current = saveLayout;

    // ── Drag interaction ───────────────────────────────────────────────────
    app.stage.on('pointerdown', (e: PIXI.FederatedPointerEvent) => {
      // Resume simulation on any click when paused
      if (pausedRef.current) {
        pausedRef.current = false;
        notifyLayoutCleanRef.current?.(false);
        settleFrames = 0;
      }

      const { x, y } = e.global;
      const worldY = y - VIEW_TOP_MARGIN_PX;
      const hit = queryPointPx(world, x, worldY);
      if (hit) {
        const pos = hit.getPosition();
        drag = { body: hit, ox: x - toPixels(pos.x), oy: worldY - toPixels(pos.y) };
        hit.setType('kinematic');
        hit.setLinearVelocity(Vec2(0, 0));
        hit.setAngularVelocity(0);
        if (pauseOnDragRef.current) {
          pausedRef.current = true;
        }
      }
    });

    app.stage.on('pointermove', (e: PIXI.FederatedPointerEvent) => {
      if (!drag) return;
      const { x, y } = e.global;
      const worldY = y - VIEW_TOP_MARGIN_PX;
      drag.body.setPosition(Vec2(toPhysics(x - drag.ox), toPhysics(worldY - drag.oy)));
    });

    const endDrag = () => {
      if (!drag) return;
      drag.body.setType('dynamic');
      drag.body.setLinearVelocity(Vec2(0, 0));
      drag.body.setAngularVelocity(0);
      drag.body.setAwake(true);
      drag = null;
      pausedRef.current = false;
      settleFrames = 0;
    };
    app.stage.on('pointerup', endDrag);
    app.stage.on('pointerupoutside', endDrag);

    const canvas = app.view as HTMLCanvasElement;
    const onWheel = (e: WheelEvent) => {
      if (!drag) return;
      e.preventDefault();
      drag.body.setAngle(drag.body.getAngle() + e.deltaY * 0.003);
    };
    canvas.addEventListener('wheel', onWheel, { passive: false });

    // ── Ticker ─────────────────────────────────────────────────────────────
    app.ticker.add(() => {
      if (!pausedRef.current) {
        // Cap delta to avoid physics explosion after tab becomes active again
        const deltaSec = Math.min(app.ticker.deltaMS, 50) / 1000;
        world.step(deltaSec, 8, 3);
      }

      renderEntries(computeOverlapping(entries));

      // Settling detection (skip when dragging or already paused)
      if (!pausedRef.current && !drag && dynamicBodies.length > 0) {
        const allSlow = dynamicBodies.every((b) => {
          const vel = b.getLinearVelocity();
          const speed = Math.sqrt(vel.x * vel.x + vel.y * vel.y);
          return speed < SPEED_THRESH && Math.abs(b.getAngularVelocity()) < ANGULAR_THRESH;
        });
        if (allSlow) {
          settleFrames++;
          if (settleFrames >= SETTLE_FRAMES) {
            pausedRef.current = true;
            if (shouldAutosave) {
              saveLayout();
            }
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
      worldRef.current = null;
      entriesRef.current = [];
      dynamicBodiesRef.current = [];
      modelColorRef.current = new Map();
      saveLayoutRef.current = null;
      app.destroy(true, { children: true, texture: true, baseTexture: true });
      // planck world is GC'd when ref is released — no explicit clear needed
    };
  }, [spawnOrder, hullMode, resetCount]);

  // ── Incremental effect: add/remove bodies when item counts change ──────────
  useEffect(() => {
    const app = appRef.current;
    const world = worldRef.current;
    if (!app || !world) return; // simulation not yet initialised

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
          const spawnX = getIncrementalSpawnX(
            spawnOrder,
            model,
            currentModels,
            currItems,
            entriesRef.current,
            hullModeRef.current,
          );

          const { body, visualLocalVerts, borderLocalVerts } = createModelBody(
            world,
            spawnX,
            spawnY,
            metrics.localVerts,
            metrics.borderLocalVerts,
            model,
          );

          spawnY -= 50 + Math.max(metrics.boundingHeightPx, 60);

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
            borderLocalVerts,
          });
        }

        pausedRef.current = false; // let new bodies fall in
        notifyLayoutCleanRef.current?.(false);
      } else {
        // Remove the models at the top of the build plate first
        for (let i = prevQty; i > currQty; i--) {
          let minY = Infinity;
          let toRemove: Entry | null = null;
          for (const e of entriesRef.current) {
            if (e.modelId === modelId && toPixels(e.body.getPosition().y) < minY) {
              minY = toPixels(e.body.getPosition().y);
              toRemove = e;
            }
          }
          if (!toRemove) break;

          world.destroyBody(toRemove.body);
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
              setHasSavedLayout(false);
              setUndoLayoutData(null);
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
              setHasSavedLayout(false);
              setUndoLayoutData(null);
              onHullModeChange?.(next);
            }}
          >
            <MenuItem value="convex">Convex hull</MenuItem>
            <MenuItem value="sansRaft">Sans raft hull</MenuItem>
          </Select>
        </FormControl>
        <Button
          size="large"
          variant={isLayoutClean ? 'outlined' : 'primary'}
          onClick={() => {
            pausedRef.current = true;
            saveLayoutRef.current?.();
          }}
          disabled={isLayoutClean}
        >
          Save
        </Button>
        <Button
          size="large"
          variant="outlined"
          onClick={() => {
            if (undoLayoutData !== null) {
              localStorage.setItem(LAYOUT_LOCALSTORAGE_KEY, undoLayoutData);
              setHasSavedLayout(true);
              setIsLayoutClean(true);
              setUndoLayoutData(null);
            }
            setResetCount((c) => c + 1);
          }}
          disabled={undoLayoutData === null && (isLayoutClean || !hasSavedLayout)}
        >
          Undo
        </Button>
        <Button
          size="large"
          variant="outlined"
          onClick={() => {
            const existing = localStorage.getItem(LAYOUT_LOCALSTORAGE_KEY);
            if (existing) setUndoLayoutData(existing);
            localStorage.removeItem(LAYOUT_LOCALSTORAGE_KEY);
            setHasSavedLayout(false);
            setIsLayoutClean(false);
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
