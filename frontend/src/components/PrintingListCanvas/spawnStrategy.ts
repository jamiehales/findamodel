import { BODY_MARGIN_PX, toPixels } from './constants';
import type { ModelFootprintMetrics, SpawnPlanItem, Entry, Vec2Like } from './types';
import type { Model, SpawnType, HullMode } from '../../lib/api';
import { getBounds, getModelFootprintMetrics, getRectLocalVerts } from './hullHelpers';
import { edgeTargets, halfAround } from './edgeLayout';

export function getRandomSpawnX(canvasWidthPx: number): number {
  return canvasWidthPx * 0.2 + Math.random() * canvasWidthPx * 0.6;
}

// Centre spawns jitter by up to this many pixels so bodies never sit exactly on top of
// each other, which gives the solver no direction to separate them in.
const CENTRE_SPAWN_JITTER_PX = 2;

/** A spawn point in the middle of the plate. */
export function centreSpawnPoint(canvasWidthPx: number, canvasHeightPx: number): Vec2Like {
  const jitter = () => (Math.random() - 0.5) * 2 * CENTRE_SPAWN_JITTER_PX;
  return { x: canvasWidthPx / 2 + jitter(), y: canvasHeightPx / 2 + jitter() };
}

interface Footprint {
  /** Centre of the footprint relative to the body's origin. */
  offset: Vec2Like;
  /** Half extents of the footprint that must stay on the plate. */
  half: Vec2Like;
  /** Half extents including the gap kept to neighbouring models. */
  spacingHalf: Vec2Like;
}

/** Body-local footprint box of a planned model, in pixels. */
function footprint(item: SpawnPlanItem): Footprint {
  const box = (verts: Vec2Like[], margin: number) => {
    const b = getBounds(verts);
    return {
      centre: { x: (b.minX + b.maxX) / 2, y: (b.minY + b.maxY) / 2 },
      half: { x: (b.maxX - b.minX) / 2 + margin, y: (b.maxY - b.minY) / 2 + margin },
    };
  };
  const border = box(item.metrics.borderLocalVerts ?? getRectLocalVerts(item.model), 0);
  const spacing = box(item.metrics.localVerts ?? getRectLocalVerts(item.model), BODY_MARGIN_PX);
  const around = halfAround(border.centre, spacing);
  return {
    offset: border.centre,
    half: border.half,
    spacingHalf: {
      x: Math.max(border.half.x, around.x),
      y: Math.max(border.half.y, around.y),
    },
  };
}

/**
 * Body positions for the "bottom edge" spawn type: rows along the bottom of the plate,
 * tallest models first, each row centred and stacked on the one below. Rows that don't fit
 * continue above the plate, so no two models start out overlapping.
 */
export function bottomEdgeSpawnPoints(
  plan: SpawnPlanItem[],
  canvasWidthPx: number,
  canvasHeightPx: number,
): Vec2Like[] {
  const boxes = plan.map(footprint);
  const order = plan
    .map((_, i) => i)
    .sort((a, b) => boxes[b].spacingHalf.y - boxes[a].spacingHalf.y);
  const points: Vec2Like[] = new Array(plan.length);
  let rowBase = canvasHeightPx;
  let start = 0;
  while (start < order.length) {
    // Fill the row left to right, always taking at least one model.
    let width = 0;
    let end = start;
    while (end < order.length) {
      const w = 2 * boxes[order[end]].spacingHalf.x;
      if (end > start && width + w > canvasWidthPx) break;
      width += w;
      end++;
    }
    let cursor = Math.max(0, (canvasWidthPx - width) / 2);
    let rowHeight = 0;
    for (const i of order.slice(start, end)) {
      const { offset, spacingHalf } = boxes[i];
      points[i] = {
        x: cursor + spacingHalf.x - offset.x,
        y: rowBase - spacingHalf.y - offset.y,
      };
      cursor += 2 * spacingHalf.x;
      rowHeight = Math.max(rowHeight, 2 * spacingHalf.y);
    }
    rowBase -= rowHeight;
    start = end;
  }
  return points;
}

/**
 * Body positions for the "edges" spawn type: each model in its slot around the plate edges
 * (see edgeTargets), in plan order clockwise from the top. Models that don't fit around the
 * edges go in rings inside, each against the ring outside it, and any left over after that
 * are stacked above the plate, so no two models start out overlapping.
 */
export function edgeSpawnPoints(
  plan: SpawnPlanItem[],
  canvasWidthPx: number,
  canvasHeightPx: number,
): Vec2Like[] {
  const boxes = plan.map(footprint);
  const centres: Array<Vec2Like | null> = plan.map(() => null);
  let rect = { left: 0, top: 0, right: canvasWidthPx, bottom: canvasHeightPx };
  let remaining = plan.map((_, i) => i);
  for (let ring = 0; remaining.length > 0; ring++) {
    const width = rect.right - rect.left;
    const height = rect.bottom - rect.top;
    if (width <= 0 || height <= 0) break;
    // Inner rings keep the outer ring's gap by treating each model's spacing box as its
    // footprint. Models too big for the ring are left for the overflow, so they don't keep
    // smaller ones out of it.
    const halfOf = (i: number) => (ring === 0 ? boxes[i].half : boxes[i].spacingHalf);
    const candidates = remaining.filter(
      (i) => 2 * halfOf(i).x <= width && 2 * halfOf(i).y <= height,
    );
    // Seed positions on a ring around the centre set the order around the plate.
    const items = candidates.map((i, k) => {
      const angle = -Math.PI / 2 + (2 * Math.PI * k) / candidates.length;
      return {
        centre: {
          x: width / 2 + (Math.cos(angle) * width) / 4,
          y: height / 2 + (Math.sin(angle) * height) / 4,
        },
        half: halfOf(i),
        spacingHalf: boxes[i].spacingHalf,
      };
    });
    const targets = edgeTargets(items, width, height);
    if (targets.every((t) => !t)) break;

    // The next ring starts inside the spacing boxes of the models touching each side.
    const inner = { ...rect };
    const touches = (a: number, b: number) => Math.abs(a - b) < 1e-6;
    targets.forEach((t, k) => {
      if (!t) return;
      const { half, spacingHalf } = items[k];
      const centre = { x: rect.left + t.x, y: rect.top + t.y };
      centres[candidates[k]] = centre;
      if (touches(t.x - half.x, 0)) inner.left = Math.max(inner.left, centre.x + spacingHalf.x);
      if (touches(t.x + half.x, width))
        inner.right = Math.min(inner.right, centre.x - spacingHalf.x);
      if (touches(t.y - half.y, 0)) inner.top = Math.max(inner.top, centre.y + spacingHalf.y);
      if (touches(t.y + half.y, height)) {
        inner.bottom = Math.min(inner.bottom, centre.y - spacingHalf.y);
      }
    });
    rect = inner;
    remaining = remaining.filter((i) => !centres[i]);
  }

  let aboveBottom = 0;
  return plan.map((_, i) => {
    const { offset, spacingHalf } = boxes[i];
    let centre = centres[i];
    if (!centre) {
      centre = { x: canvasWidthPx / 2, y: aboveBottom - spacingHalf.y };
      aboveBottom -= 2 * spacingHalf.y;
    }
    return { x: centre.x - offset.x, y: centre.y - offset.y };
  });
}

export function clampSpawnX(spawnX: number, widthPx: number, canvasWidthPx: number): number {
  const margin = widthPx / 2 + BODY_MARGIN_PX + 8;
  return Math.min(canvasWidthPx - margin, Math.max(margin, spawnX));
}

export function compareBySizeDesc(a: ModelFootprintMetrics, b: ModelFootprintMetrics): number {
  return b.boundingAreaPx2 - a.boundingAreaPx2 || b.footprintAreaPx2 - a.footprintAreaPx2;
}

export function buildSpawnPlan(
  models: Model[],
  items: Record<string, number>,
  spawnOrder: SpawnType,
  hullMode: HullMode,
  canvasWidthPx: number,
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
      spawnX: clampSpawnX(getRandomSpawnX(canvasWidthPx), metrics.boundingWidthPx, canvasWidthPx),
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
    const anchorX = clampSpawnX(
      getRandomSpawnX(canvasWidthPx),
      largest.metrics.boundingWidthPx,
      canvasWidthPx,
    );

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
        spawnX: clampSpawnX(anchorX, filler.metrics.boundingWidthPx, canvasWidthPx),
      });
    }
  }

  return plan;
}

export function getIncrementalSpawnX(
  spawnOrder: SpawnType,
  model: Model,
  models: Model[],
  items: Record<string, number>,
  entries: Entry[],
  hullMode: HullMode,
  canvasWidthPx: number,
): number {
  const currentMetrics = getModelFootprintMetrics(model, hullMode);
  if (spawnOrder !== 'largestFirstFillGaps') {
    return clampSpawnX(
      getRandomSpawnX(canvasWidthPx),
      currentMetrics.boundingWidthPx,
      canvasWidthPx,
    );
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
    return clampSpawnX(
      getRandomSpawnX(canvasWidthPx),
      currentMetrics.boundingWidthPx,
      canvasWidthPx,
    );
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
        canvasWidthPx,
      );
    }
  }

  return clampSpawnX(getRandomSpawnX(canvasWidthPx), currentMetrics.boundingWidthPx, canvasWidthPx);
}
