import { World, Vec2, Polygon, Box, Settings } from 'planck';
import type { Body } from 'planck';
import { CAT_OBJECT, CAT_BORDER, CAT_WALL, BODY_MARGIN_PX, toPhysics } from './constants';
import type { Vec2Like } from './types';
import type { Model } from '../../lib/api';
import { getRectLocalVerts, getBounds, inflateVerts, toPhysicsVerts } from './hullHelpers';

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
 * Create a dynamic body with two fixtures:
 *  1. Object-object collision fixture (spawn hull, inflated for gap)
 *  2. Border collision fixture (always with-raft hull, not inflated)
 *
 * Returns the body and the visual/border local verts in pixel space (for rendering).
 */
export function createModelBody(
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
