import { PX_PER_MM, BODY_MARGIN_PX, toPhysics } from './constants';
import type { Vec2Like, ModelFootprintMetrics } from './types';
import type { Model, HullMode } from '../../lib/api';

/**
 * Parse a JSON hull string ("[[x,z],[x,z],...]") into local-space pixel
 * vertices centred at the origin
 */
export function parseHullLocalPx(hullJson: string | null): Vec2Like[] | null {
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

export function getSpawnHullJson(model: Model, hullMode: HullMode): string | null {
  return hullMode === 'sansRaft'
    ? (model.convexSansRaftHull ?? model.convexHull)
    : model.convexHull;
}

/** Border hull always uses the full convex hull (with raft). */
export function getBorderHullJson(model: Model): string | null {
  return model.convexHull;
}

export function getFillEstimateHullJson(model: Model, hullMode: HullMode): string | null {
  return model.concaveHull ?? getSpawnHullJson(model, hullMode);
}

export function polygonArea(verts: Vec2Like[]): number {
  if (verts.length < 3) return 0;

  let area = 0;
  for (let i = 0; i < verts.length; i++) {
    const a = verts[i];
    const b = verts[(i + 1) % verts.length];
    area += a.x * b.y - b.x * a.y;
  }

  return Math.abs(area) / 2;
}

export function getBounds(verts: Vec2Like[]): {
  minX: number;
  maxX: number;
  minY: number;
  maxY: number;
} {
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

export function getRectLocalVerts(model: Model): Vec2Like[] {
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

export function getModelFootprintMetrics(model: Model, hullMode: HullMode): ModelFootprintMetrics {
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

/**
 * Expand each vertex outward by `amount` px along its vertex normal.
 * The vertex normal is the average of the outward-facing normals of the two
 * adjacent edges, making this a true parallel offset for convex hulls.
 * Winding-order agnostic: the result is flipped to face away from origin when
 * needed (verts must be centred at origin, as parseHullLocalPx guarantees).
 */
export function inflateVerts(verts: Vec2Like[], amount: number): Vec2Like[] {
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
export function ensurePlanckWinding(verts: Vec2Like[]): Vec2Like[] {
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
export function toPhysicsVerts(pxVerts: Vec2Like[]): Vec2Like[] {
  return ensurePlanckWinding(pxVerts).map((v) => ({ x: toPhysics(v.x), y: toPhysics(v.y) }));
}

// Re-export BODY_MARGIN_PX so physicsWorld can import it from here rather than constants
export { BODY_MARGIN_PX };
