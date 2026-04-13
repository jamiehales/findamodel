import { BODY_MARGIN_PX, toPixels } from './constants';
import type { ModelFootprintMetrics, SpawnPlanItem, Entry } from './types';
import type { Model, SpawnType, HullMode } from '../../lib/api';
import { getModelFootprintMetrics } from './hullHelpers';

export function getRandomSpawnX(canvasWidthPx: number): number {
  return canvasWidthPx * 0.2 + Math.random() * canvasWidthPx * 0.6;
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
