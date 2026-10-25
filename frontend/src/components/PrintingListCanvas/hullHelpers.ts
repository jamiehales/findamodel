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
  switch (hullMode) {
    case 'sansRaft':
      return model.convexSansRaftHull ?? model.convexHull;
    case 'concave':
      return model.concaveHull ?? model.convexHull;
    default:
      return model.convexHull;
  }
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

/** Twice the signed area; positive for math-CCW (screen-CW) winding. */
function signedArea2(verts: Vec2Like[]): number {
  let area = 0;
  for (let i = 0; i < verts.length; i++) {
    const a = verts[i];
    const b = verts[(i + 1) % verts.length];
    area += a.x * b.y - b.x * a.y;
  }
  return area;
}

function turn(o: Vec2Like, a: Vec2Like, b: Vec2Like): number {
  return (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
}

function isConvex(verts: Vec2Like[], sign: number): boolean {
  const n = verts.length;
  for (let i = 0; i < n; i++) {
    if (turn(verts[(i - 1 + n) % n], verts[i], verts[(i + 1) % n]) * sign < -1e-9) return false;
  }
  return true;
}

function inTriangle(p: Vec2Like, a: Vec2Like, b: Vec2Like, c: Vec2Like): boolean {
  const d1 = turn(a, b, p);
  const d2 = turn(b, c, p);
  const d3 = turn(c, a, p);
  const hasNeg = d1 < 0 || d2 < 0 || d3 < 0;
  const hasPos = d1 > 0 || d2 > 0 || d3 > 0;
  return !(hasNeg && hasPos);
}

/**
 * Split a simple polygon into convex pieces, since planck.js polygons must be
 * convex. Ear clipping gives triangles, which are then merged across shared
 * edges wherever the result stays convex (Hertel-Mehlhorn). A convex polygon,
 * or one that can't be clipped, is returned whole.
 */
export function decomposeConvex(verts: Vec2Like[]): Vec2Like[][] {
  const sign = signedArea2(verts) >= 0 ? 1 : -1;
  if (verts.length <= 3 || isConvex(verts, sign)) return [verts];

  const remaining = verts.map((_, i) => i);
  const pieces: number[][] = [];
  while (remaining.length > 3) {
    const m = remaining.length;
    let clipped = false;
    for (let k = 0; k < m; k++) {
      const [ia, ib, ic] = [remaining[(k - 1 + m) % m], remaining[k], remaining[(k + 1) % m]];
      const [a, b, c] = [verts[ia], verts[ib], verts[ic]];
      if (turn(a, b, c) * sign <= 0) continue;
      const blocked = remaining.some(
        (j) => j !== ia && j !== ib && j !== ic && inTriangle(verts[j], a, b, c),
      );
      if (blocked) continue;
      pieces.push([ia, ib, ic]);
      remaining.splice(k, 1);
      clipped = true;
      break;
    }
    if (!clipped) return [verts];
  }
  pieces.push(remaining);

  // Merge pieces that share an edge (a→b in one, b→a in the other).
  let merged = true;
  while (merged) {
    merged = false;
    search: for (let p = 0; p < pieces.length; p++) {
      for (let q = p + 1; q < pieces.length; q++) {
        const first = pieces[p];
        const second = pieces[q];
        for (let i = 0; i < first.length; i++) {
          const a = first[i];
          const b = first[(i + 1) % first.length];
          const j = second.findIndex(
            (v, idx) => v === b && second[(idx + 1) % second.length] === a,
          );
          if (j < 0) continue;
          // first rotated to run b … a, then second's vertices strictly between a and b.
          const fromB = [...first.slice(i + 1), ...first.slice(0, i + 1)];
          const fromA = [...second.slice(j + 1), ...second.slice(0, j + 1)];
          const candidate = [...fromB, ...fromA.slice(1, -1)];
          if (
            !isConvex(
              candidate.map((v) => verts[v]),
              sign,
            )
          )
            continue;
          pieces[p] = candidate;
          pieces.splice(q, 1);
          merged = true;
          break search;
        }
      }
    }
  }
  return pieces.map((piece) => piece.map((v) => verts[v]));
}

/**
 * Expand each vertex outward by `amount` px along its vertex normal.
 * The vertex normal is the average of the outward-facing normals of the two
 * adjacent edges, making this a true parallel offset for convex polygons.
 * Outward is taken from the winding order, so the polygon need not contain
 * the origin.
 */
export function inflateVerts(verts: Vec2Like[], amount: number): Vec2Like[] {
  const n = verts.length;
  const sign = signedArea2(verts) >= 0 ? 1 : -1;
  return verts.map((v, i) => {
    const prev = verts[(i - 1 + n) % n];
    const next = verts[(i + 1) % n];

    // Perpendicular (rotated 90° CW) to each adjacent edge.
    const nx = v.y - prev.y + (next.y - v.y);
    const ny = -(v.x - prev.x) + -(next.x - v.x);

    const len = Math.sqrt(nx * nx + ny * ny);
    if (len < 0.001) return v;

    return { x: v.x + ((sign * nx) / len) * amount, y: v.y + ((sign * ny) / len) * amount };
  });
}

/**
 * Ensure vertices are in the winding order that planck.js (Box2D) expects for
 * valid convex polygons.  In our Y-down screen coordinate system, planck needs
 * the signed area to be positive (screen-CW = math-CCW).
 */
export function ensurePlanckWinding(verts: Vec2Like[]): Vec2Like[] {
  if (signedArea2(verts) < 0) return [...verts].reverse();
  return verts;
}

/** Convert local pixel-space vertices to planck Vec2 in physics units. */
export function toPhysicsVerts(pxVerts: Vec2Like[]): Vec2Like[] {
  return ensurePlanckWinding(pxVerts).map((v) => ({ x: toPhysics(v.x), y: toPhysics(v.y) }));
}

// Re-export BODY_MARGIN_PX so physicsWorld can import it from here rather than constants
export { BODY_MARGIN_PX };
