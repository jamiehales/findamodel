import { World, Vec2, Polygon, Box, Settings, AABB, Transform } from 'planck';
import type { Body } from 'planck';
import { CAT_OBJECT, CAT_BORDER, CAT_WALL, BODY_MARGIN_PX, toPhysics } from './constants';
import type { Vec2Like } from './types';
import type { Model } from '../../lib/api';
import {
  getRectLocalVerts,
  getBounds,
  inflateVerts,
  toPhysicsVerts,
  decomposeConvex,
} from './hullHelpers';

// Allow complex convex hulls with many vertices
Settings.maxPolygonVertices = 64;

export const FIXTURE_OPT_OBJECT = {
  density: 1.0,
  restitution: 0.05,
  friction: 0.5,
  filterCategoryBits: CAT_OBJECT,
  filterMaskBits: CAT_OBJECT,
};

export const FIXTURE_OPT_BORDER = {
  density: 0, // border fixture doesn't affect mass
  restitution: 0.05,
  friction: 0.5,
  filterCategoryBits: CAT_BORDER,
  filterMaskBits: CAT_WALL,
};

export const BODY_DEF_DYNAMIC = {
  type: 'dynamic' as const,
  linearDamping: 1.5,
  angularDamping: 1.5,
};

/**
 * Create a dynamic body with two kinds of fixture:
 *  1. Object-object collision fixtures (spawn hull, inflated for gap). A concave
 *     spawn hull gets one fixture per convex piece.
 *  2. Border collision fixture (always with-raft hull, not inflated)
 *
 * Returns the body, the visual/border local verts in pixel space (for rendering)
 * and the convex pieces of the visual verts (for overlap tests).
 */
export function createModelBody(
  world: InstanceType<typeof World>,
  cxPx: number,
  cyPx: number,
  spawnLocalVerts: Vec2Like[] | null,
  borderLocalVerts: Vec2Like[] | null,
  model: Model,
): {
  body: Body;
  visualLocalVerts: Vec2Like[];
  visualPieces: Vec2Like[][];
  borderLocalVerts: Vec2Like[];
} {
  const body = world.createDynamicBody({
    ...BODY_DEF_DYNAMIC,
    position: Vec2(toPhysics(cxPx), toPhysics(cyPx)),
  });

  let visualLocal: Vec2Like[];
  let visualPieces: Vec2Like[][];
  let borderLocal: Vec2Like[];

  // ── Object-object fixtures (spawn hull) ─────────────────────────────────
  if (spawnLocalVerts && spawnLocalVerts.length >= 3) {
    const pieces = decomposeConvex(spawnLocalVerts);
    const created: ReturnType<Body['createFixture']>[] = [];
    visualLocal = spawnLocalVerts;
    try {
      for (const piece of pieces) {
        const inflated = inflateVerts(piece, BODY_MARGIN_PX);
        created.push(body.createFixture(new Polygon(toPhysicsVerts(inflated)), FIXTURE_OPT_OBJECT));
      }
      visualPieces = pieces;
    } catch {
      created.forEach((f) => body.destroyFixture(f));
      visualPieces = [];
    }
    if (visualPieces.length === 0 && pieces.length > 1) {
      // A piece was rejected - fall back to the whole outline, which planck makes convex
      try {
        const inflated = inflateVerts(spawnLocalVerts, BODY_MARGIN_PX);
        body.createFixture(new Polygon(toPhysicsVerts(inflated)), FIXTURE_OPT_OBJECT);
        visualPieces = [spawnLocalVerts];
      } catch {
        // Falls through to the rectangle below
      }
    }
    if (visualPieces.length === 0) {
      // Hull rejected by planck – fall back to rectangle
      const rectVerts = getRectLocalVerts(model);
      const bounds = getBounds(rectVerts);
      const hw = toPhysics((bounds.maxX - bounds.minX) / 2 + BODY_MARGIN_PX);
      const hh = toPhysics((bounds.maxY - bounds.minY) / 2 + BODY_MARGIN_PX);
      body.createFixture(new Box(hw, hh), FIXTURE_OPT_OBJECT);
      visualLocal = rectVerts;
      visualPieces = [rectVerts];
    }
  } else {
    const rectVerts = getRectLocalVerts(model);
    const bounds = getBounds(rectVerts);
    const hw = toPhysics((bounds.maxX - bounds.minX) / 2 + BODY_MARGIN_PX);
    const hh = toPhysics((bounds.maxY - bounds.minY) / 2 + BODY_MARGIN_PX);
    body.createFixture(new Box(hw, hh), FIXTURE_OPT_OBJECT);
    visualLocal = rectVerts;
    visualPieces = [rectVerts];
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

  return { body, visualLocalVerts: visualLocal, visualPieces, borderLocalVerts: borderLocal };
}

/**
 * Bounding box (centre and half extents, physics units) of the body's fixtures in the given
 * collision category, at its current pose. Null when it has none.
 */
export function fixtureBounds(
  body: Body,
  category: number,
): { centre: Vec2Like; half: Vec2Like } | null {
  const xf = body.getTransform();
  const box = AABB();
  let lower: Vec2Like | null = null;
  let upper: Vec2Like | null = null;
  for (let f = body.getFixtureList(); f; f = f.getNext()) {
    if (f.getFilterCategoryBits() !== category) continue;
    f.getShape().computeAABB(box, xf, 0);
    lower = lower
      ? { x: Math.min(lower.x, box.lowerBound.x), y: Math.min(lower.y, box.lowerBound.y) }
      : { x: box.lowerBound.x, y: box.lowerBound.y };
    upper = upper
      ? { x: Math.max(upper.x, box.upperBound.x), y: Math.max(upper.y, box.upperBound.y) }
      : { x: box.upperBound.x, y: box.upperBound.y };
  }
  if (!lower || !upper) return null;
  return {
    centre: { x: (lower.x + upper.x) / 2, y: (lower.y + upper.y) / 2 },
    half: { x: (upper.x - lower.x) / 2, y: (upper.y - lower.y) / 2 },
  };
}

/**
 * Whether `body` could move to `target` (physics units) without its object fixtures'
 * bounding boxes touching another body's. Boxes come from the shapes, not the broad
 * phase, whose boxes stay stretched along a sleeping body's last motion.
 */
export function isSpotClear(
  world: InstanceType<typeof World>,
  body: Body,
  target: Vec2Like,
): boolean {
  const moved = Transform(Vec2(target.x, target.y), body.getAngle());
  const box = AABB();
  const otherBox = AABB();
  for (let f = body.getFixtureList(); f; f = f.getNext()) {
    if (f.getFilterCategoryBits() !== CAT_OBJECT) continue;
    f.getShape().computeAABB(box, moved, 0);
    let clear = true;
    world.queryAABB(box, (other) => {
      if (other.getBody() === body || other.getFilterCategoryBits() !== CAT_OBJECT) return true;
      other.getShape().computeAABB(otherBox, other.getBody().getTransform(), 0);
      if (!AABB.testOverlap(box, otherBox)) return true;
      clear = false;
      return false;
    });
    if (!clear) return false;
  }
  return true;
}

/** Find the first dynamic body whose fixture contains the given pixel-space point. */
export function queryPointPx(
  world: InstanceType<typeof World>,
  xPx: number,
  yPx: number,
): Body | null {
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
