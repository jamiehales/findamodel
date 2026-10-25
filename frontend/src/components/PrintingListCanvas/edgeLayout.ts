import type { Vec2Like } from './types';

export interface EdgeItem {
  /** Current centre of the model's footprint. */
  centre: Vec2Like;
  /** Half extents of the footprint that must stay on the plate. */
  half: Vec2Like;
  /** Half extents including the gap kept to neighbouring models. */
  spacingHalf: Vec2Like;
}

/**
 * Position of `p` around the plate centre, clockwise from the top-left corner (screen
 * space, y down), as the distance along the plate edge where the ray from the centre
 * through `p` meets it.
 */
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

/** How far inside the plate the item's footprint is from the nearest edge. */
function distanceToEdge(item: EdgeItem, width: number, height: number): number {
  return Math.min(
    item.centre.x - item.half.x,
    width - item.centre.x - item.half.x,
    item.centre.y - item.half.y,
    height - item.centre.y - item.half.y,
  );
}

interface Spread {
  /** Centres along the side, from 0. */
  centres: number[];
  /** Gap between neighbouring spacing boxes (Infinity when there are no neighbours). */
  gap: number;
}

/**
 * A row spanning `length`: the end items touch the ends and the gaps between neighbours'
 * spacing boxes are equal. Null when the row doesn't fit.
 */
function spreadRow(half: number[], spacingHalf: number[], length: number): Spread | null {
  const n = half.length;
  if (n === 0) return { centres: [], gap: Infinity };
  if (n === 1) return 2 * half[0] <= length ? { centres: [length / 2], gap: Infinity } : null;
  let inner = 0;
  for (let i = 0; i + 1 < n; i++) inner += spacingHalf[i] + spacingHalf[i + 1];
  const gap = (length - half[0] - half[n - 1] - inner) / (n - 1);
  if (gap < 0) return null;
  const centres = [half[0]];
  for (let i = 1; i < n; i++) {
    centres.push(centres[i - 1] + spacingHalf[i - 1] + gap + spacingHalf[i]);
  }
  return { centres, gap };
}

/**
 * A column between two rows: equal gaps between spacing boxes, including at both ends.
 * Null when it doesn't fit.
 */
function spreadColumn(spacingHalf: number[], length: number): Spread | null {
  if (spacingHalf.length === 0) return { centres: [], gap: Infinity };
  const total = spacingHalf.reduce((sum, h) => sum + 2 * h, 0);
  const gap = (length - total) / (spacingHalf.length + 1);
  if (gap < 0) return null;
  const centres: number[] = [];
  let cursor = gap;
  for (const h of spacingHalf) {
    centres.push(cursor + h);
    cursor += 2 * h + gap;
  }
  return { centres, gap };
}

interface Layout {
  targets: Map<number, Vec2Like>;
  /** Smallest gap between neighbours on any side. */
  minGap: number;
  /** Total squared distance the items move. */
  cost: number;
}

/**
 * Lays `seq` (item indices in clockwise order) out with the given number of items per side:
 * top left to right, right top to bottom, bottom right to left, left bottom to top. The top
 * and bottom rows run the full width with their end items in the corners; the side
 * columns fill the space between the rows.
 */
function layoutSides(
  items: EdgeItem[],
  seq: number[],
  counts: [number, number, number, number],
  width: number,
  height: number,
): Layout | null {
  const [cTop, cRight, cBottom] = counts;
  const top = seq.slice(0, cTop);
  const right = seq.slice(cTop, cTop + cRight);
  const bottom = seq.slice(cTop + cRight, cTop + cRight + cBottom);
  const left = seq.slice(cTop + cRight + cBottom);

  const row = (side: number[]) =>
    spreadRow(
      side.map((i) => items[i].half.x),
      side.map((i) => items[i].spacingHalf.x),
      width,
    );
  const topRow = row(top);
  const bottomRow = row(bottom);
  if (!topRow || !bottomRow) return null;

  // Side columns start below the top row's spacing boxes and end above the bottom row's.
  const rowDepth = (side: number[]) =>
    Math.max(0, ...side.map((i) => items[i].half.y + items[i].spacingHalf.y));
  const rowTop = rowDepth(top);
  const rowBottom = rowDepth(bottom);
  const column = (side: number[]) =>
    spreadColumn(
      side.map((i) => items[i].spacingHalf.y),
      height - rowTop - rowBottom,
    );
  const rightColumn = column(right);
  const leftColumn = column(left);
  if (!rightColumn || !leftColumn) return null;

  const targets = new Map<number, Vec2Like>();
  top.forEach((i, k) => targets.set(i, { x: topRow.centres[k], y: items[i].half.y }));
  right.forEach((i, k) =>
    targets.set(i, { x: width - items[i].half.x, y: rowTop + rightColumn.centres[k] }),
  );
  bottom.forEach((i, k) =>
    targets.set(i, { x: width - bottomRow.centres[k], y: height - items[i].half.y }),
  );
  left.forEach((i, k) =>
    targets.set(i, { x: items[i].half.x, y: height - rowBottom - leftColumn.centres[k] }),
  );

  let cost = 0;
  for (const [i, t] of targets) {
    cost += (t.x - items[i].centre.x) ** 2 + (t.y - items[i].centre.y) ** 2;
  }
  const gaps = [topRow.gap, rightColumn.gap, bottomRow.gap, leftColumn.gap].filter(Number.isFinite);
  return { targets, minGap: gaps.length ? Math.min(...gaps) : Infinity, cost };
}

/** Within this fraction of the roomiest arrangement, the one moving items least wins. */
const SPACING_TOLERANCE = 0.1;

/**
 * Whether `a` is a better arrangement than `b`: a larger smallest gap (so every item gets
 * as much room as possible, which spreads them evenly), then less movement.
 */
function isBetter(a: Layout, b: Layout): boolean {
  const tolerance = SPACING_TOLERANCE * Math.max(a.minGap, b.minGap, 1e-9);
  if (Math.abs(a.minGap - b.minGap) > tolerance) return a.minGap > b.minGap;
  return a.cost < b.cost;
}

/** Per-side counts to try for `n` items: every split with each side's share near even. */
function candidateCounts(n: number): Array<[number, number, number, number]> {
  const out: Array<[number, number, number, number]> = [];
  for (let cTop = 0; cTop <= n; cTop++) {
    for (let cBottom = 0; cBottom <= n - cTop; cBottom++) {
      if (Math.abs(cTop - cBottom) > 1) continue;
      const sides = n - cTop - cBottom;
      const cRight = Math.floor(sides / 2);
      out.push([cTop, cRight, cBottom, sides - cRight]);
      if (sides % 2 === 1) out.push([cTop, sides - cRight, cBottom, cRight]);
    }
  }
  return out;
}

/**
 * Footprint centres spreading the items around the plate's edges, with the gaps between
 * neighbours as even as the sides allow. Items keep their order around the plate centre,
 * and among the most even arrangements the one moving them least is used. Slots never
 * overlap, measured by each item's spacing box.
 *
 * When the items don't all fit around the edges, those furthest from an edge are left out
 * (null) until the rest fit.
 */
export function edgeTargets(
  items: EdgeItem[],
  width: number,
  height: number,
): Array<Vec2Like | null> {
  const byEdgeDistance = items
    .map((_, i) => i)
    .sort(
      (a, b) => distanceToEdge(items[a], width, height) - distanceToEdge(items[b], width, height),
    );
  const params = items.map((item) => perimeterParam(item.centre, width, height));

  const bestFor = (keep: number): Layout | null => {
    const order = byEdgeDistance.slice(0, keep).sort((a, b) => params[a] - params[b]);
    let best: Layout | null = null;
    for (const counts of candidateCounts(order.length)) {
      for (let start = 0; start < order.length; start++) {
        const seq = [...order.slice(start), ...order.slice(0, start)];
        const layout = layoutSides(items, seq, counts, width, height);
        if (layout && (!best || isBetter(layout, best))) best = layout;
      }
    }
    return best;
  };

  // The most items that fit, found by binary search (fewer items always fit more easily).
  let best = bestFor(items.length);
  if (!best) {
    let fits = 0;
    let fails = items.length;
    while (fails - fits > 1) {
      const mid = Math.floor((fits + fails) / 2);
      const layout = bestFor(mid);
      if (layout) {
        fits = mid;
        best = layout;
      } else {
        fails = mid;
      }
    }
  }
  const targets = best?.targets;
  return items.map((_, i) => targets?.get(i) ?? null);
}
