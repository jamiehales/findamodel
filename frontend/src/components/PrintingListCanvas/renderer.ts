import * as PIXI from 'pixi.js';
import { Vec2, Polygon } from 'planck';
import type { Body } from 'planck';
import { DEBUG_PHYSICS_WIREFRAME, CAT_OBJECT, toPhysics, toPixels } from './constants';
import type { Vec2Like, Entry } from './types';

export function darkenColor(color: number, factor: number): number {
  const r = Math.round(((color >> 16) & 0xff) * factor);
  const g = Math.round(((color >> 8) & 0xff) * factor);
  const b = Math.round((color & 0xff) * factor);
  return (r << 16) | (g << 8) | b;
}

/** Transform local-space pixel vertices to world pixel space using a body's pose. */
export function toWorldVerts(body: Body, localVerts: Vec2Like[]): Vec2Like[] {
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
export function satOverlap(a: Vec2Like[], b: Vec2Like[]): boolean {
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
export function computeOverlapping(entries: Entry[]): Set<Body> {
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
export function isOutOfBounds(
  body: Body,
  borderLocalVerts: Vec2Like[],
  canvasWidthPhys: number,
  canvasHeightPhys: number,
): boolean {
  if (!borderLocalVerts.length) return false;
  return borderLocalVerts.some((v) => {
    const w = body.getWorldPoint(Vec2(toPhysics(v.x), toPhysics(v.y)));
    return w.x < 0 || w.x > canvasWidthPhys || w.y < 0 || w.y > canvasHeightPhys;
  });
}

export function drawDottedRect(
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

/**
 * Draw the visual (non-inflated) hull by transforming local vertices through
 * the body's current position and angle. This keeps the rendered polygon at the
 * original model size while the physics body carries the gap margin.
 */
export function drawBody(
  gfx: PIXI.Graphics,
  body: Body,
  visualLocalVerts: Vec2Like[],
  borderLocalVerts: Vec2Like[],
  color: number,
  highlightOutOfBounds = false,
  yOffset = 0,
  scale = 1,
) {
  gfx.clear();
  if (!visualLocalVerts.length) return;

  const pos = body.getPosition();
  const px = toPixels(pos.x) * scale;
  const py = toPixels(pos.y) * scale;
  const angle = body.getAngle();
  const cos = Math.cos(angle);
  const sin = Math.sin(angle);

  gfx.beginFill(color, highlightOutOfBounds ? 0.62 : 0.45);
  gfx.lineStyle(highlightOutOfBounds ? 3 : 1.5, highlightOutOfBounds ? 0xef4444 : color, 0.95);
  const v0 = visualLocalVerts[0];
  gfx.moveTo(
    (v0.x * cos - v0.y * sin) * scale + px,
    (v0.x * sin + v0.y * cos) * scale + py + yOffset,
  );
  for (let i = 1; i < visualLocalVerts.length; i++) {
    const v = visualLocalVerts[i];
    gfx.lineTo(
      (v.x * cos - v.y * sin) * scale + px,
      (v.x * sin + v.y * cos) * scale + py + yOffset,
    );
  }
  gfx.closePath();
  gfx.endFill();

  // When out of bounds, draw a red outline using the with-raft border hull.
  if (highlightOutOfBounds && borderLocalVerts.length) {
    gfx.lineStyle(3, 0xef4444, 0.98);
    const b0 = borderLocalVerts[0];
    gfx.moveTo(
      (b0.x * cos - b0.y * sin) * scale + px,
      (b0.x * sin + b0.y * cos) * scale + py + yOffset,
    );
    for (let i = 1; i < borderLocalVerts.length; i++) {
      const v = borderLocalVerts[i];
      gfx.lineTo(
        (v.x * cos - v.y * sin) * scale + px,
        (v.x * sin + v.y * cos) * scale + py + yOffset,
      );
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
          gfx.moveTo(toPixels(wp0.x) * scale, toPixels(wp0.y) * scale + yOffset);
          for (let vi = 1; vi < vCount; vi++) {
            const wp = body.getWorldPoint(poly.m_vertices[vi]);
            gfx.lineTo(toPixels(wp.x) * scale, toPixels(wp.y) * scale + yOffset);
          }
          gfx.closePath();
        }
      }
      fixture = fixture.getNext();
    }
  }
}
