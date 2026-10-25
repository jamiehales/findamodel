import type { Vec2Like } from './types';

// Perimeter positions run clockwise from the plate's top-left corner (screen space, y
// down): top edge, right edge, bottom edge, left edge.

export interface EdgeItem {
  /** Current centre of the model's footprint. */
  centre: Vec2Like;
  /** Half extents of the footprint that must stay on the plate. */
  half: Vec2Like;
  /** Half extents including the gap kept to neighbouring models. */
  spacingHalf: Vec2Like;
}

/** Where the ray from the plate centre through `p` meets the plate edge. */
export function perimeterParam(p: Vec2Like, width: number, height: number): number {
  const cx = width / 2;
  const cy = height / 2;
  const dx = p.x - cx;
  const dy = p.y - cy;
  if (Math.abs(dx) < 1e-9 && Math.abs(dy) < 1e-9) return 2 * width + height - cx; // bottom middle
  const toSide = Math.abs(dx) < 1e-9 ? Infinity : cx / Math.abs(dx);
  const toTopOrBottom = Math.abs(dy) < 1e-9 ? Infinity : cy / Math.abs(dy);
  const scale = Math.min(toSide, toTopOrBottom);
  const x = cx + dx * scale;
  const y = cy + dy * scale;
  if (toTopOrBottom <= toSide) return dy < 0 ? x : 2 * width + height - x;
  return dx > 0 ? width + y : 2 * width + 2 * height - y;
}

/**
 * Unit direction from the plate centre toward its nearest edge, with offsets scaled by the
 * plate's half extents. Points down at the exact centre.
 */
export function awayFromCentre(p: Vec2Like, width: number, height: number): Vec2Like {
  const dx = (p.x - width / 2) / (width / 2);
  const dy = (p.y - height / 2) / (height / 2);
  const length = Math.hypot(dx, dy);
  if (length < 1e-6) return { x: 0, y: 1 };
  return { x: dx / length, y: dy / length };
}

/** The point on the plate edge at perimeter position `t`. */
export function perimeterPoint(t: number, width: number, height: number): Vec2Like {
  if (t < width) return { x: t, y: 0 };
  if (t < width + height) return { x: width, y: t - width };
  if (t < 2 * width + height) return { x: 2 * width + height - t, y: height };
  return { x: 0, y: 2 * width + 2 * height - t };
}

function isAlongTopOrBottom(t: number, width: number, height: number): boolean {
  return t < width || (t >= width + height && t < 2 * width + height);
}

function clampInto(v: number, half: number, size: number): number {
  if (2 * half >= size) return size / 2;
  return Math.min(size - half, Math.max(half, v));
}

/** How far inside the plate the item's footprint is from the nearest edge. */
function distanceToEdge(item: EdgeItem, width: number, height: number): number {
  return Math.min(
    item.centre.x - item.half.x,
    width - item.centre.x - item.half.x,
    item.centre.y - item.half.y,
    height - item.centre.y - item.half.y,
  );
}

/**
 * Footprint centres spreading the items around the plate's edges with equal gaps between
 * neighbours. Items keep their order around the plate centre, and the arrangement is
 * turned as little as possible from where they are now.
 *
 * Slots are spaced along the track the centres travel: the plate shrunk by the items'
 * average half extents, so a corner holds one item and its neighbours sit beside it
 * rather than being squeezed into it. Each target is then fitted to its own item's size.
 *
 * When the items don't all fit around the edges, those already nearest an edge get slots
 * and the rest get null, so slots never overlap.
 */
export function edgeTargets(
  items: EdgeItem[],
  width: number,
  height: number,
): Array<Vec2Like | null> {
  const n = items.length;
  if (n === 0) return [];
  const insetX = items.reduce((sum, item) => sum + item.half.x, 0) / n;
  const insetY = items.reduce((sum, item) => sum + item.half.y, 0) / n;
  const trackWidth = width - 2 * insetX;
  const trackHeight = height - 2 * insetY;
  if (trackWidth <= 0 || trackHeight <= 0) return items.map(() => null);

  const perimeter = 2 * (trackWidth + trackHeight);
  const onTrack = (p: Vec2Like) => ({ x: p.x - insetX, y: p.y - insetY });
  const params = items.map((item) => perimeterParam(onTrack(item.centre), trackWidth, trackHeight));
  const extents = items.map(
    (item, i) =>
      2 *
      (isAlongTopOrBottom(params[i], trackWidth, trackHeight)
        ? item.spacingHalf.x
        : item.spacingHalf.y),
  );

  const byEdgeDistance = items
    .map((_, i) => i)
    .sort(
      (a, b) => distanceToEdge(items[a], width, height) - distanceToEdge(items[b], width, height),
    );
  const chosen: number[] = [];
  let used = 0;
  for (const i of byEdgeDistance) {
    if (used + extents[i] > perimeter) continue;
    chosen.push(i);
    used += extents[i];
  }
  if (chosen.length === 0) return items.map(() => null);

  const order = [...chosen].sort((a, b) => params[a] - params[b]);
  const gap = (perimeter - used) / order.length;
  const slots = new Map<number, number>();
  let cursor = 0;
  for (const i of order) {
    slots.set(i, cursor + extents[i] / 2);
    cursor += extents[i] + gap;
  }

  // Turn the whole arrangement by the circular mean of how far each item is from its slot.
  let sumCos = 0;
  let sumSin = 0;
  for (const [i, slot] of slots) {
    const angle = (2 * Math.PI * (params[i] - slot)) / perimeter;
    sumCos += Math.cos(angle);
    sumSin += Math.sin(angle);
  }
  const offset = (Math.atan2(sumSin, sumCos) / (2 * Math.PI)) * perimeter;

  // Fit each target to its item: on a track side, its edge touches the plate edge.
  const eps = 1e-9;
  return items.map((item, i) => {
    const slot = slots.get(i);
    if (slot === undefined) return null;
    const t = (((slot + offset) % perimeter) + perimeter) % perimeter;
    const p = perimeterPoint(t, trackWidth, trackHeight);
    let x = p.x + insetX;
    let y = p.y + insetY;
    if (p.x <= eps) x = item.half.x;
    else if (p.x >= trackWidth - eps) x = width - item.half.x;
    if (p.y <= eps) y = item.half.y;
    else if (p.y >= trackHeight - eps) y = height - item.half.y;
    return { x: clampInto(x, item.half.x, width), y: clampInto(y, item.half.y, height) };
  });
}
