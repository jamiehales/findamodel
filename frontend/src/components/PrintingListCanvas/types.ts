import type * as PIXI from 'pixi.js';
import type { Body } from 'planck';
import type { Model, SpawnType, HullMode, PrinterConfig } from '../../lib/api';

export interface Vec2Like {
  x: number;
  y: number;
}

export interface SavedLayout {
  itemsKey: string;
  positions: {
    modelId: string;
    instanceIndex: number;
    xMm: number;
    yMm: number;
    angle: number;
  }[];
}

export interface Entry {
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

export interface ModelFootprintMetrics {
  bodyHullJson: string | null;
  borderHullJson: string | null;
  localVerts: Vec2Like[] | null;
  borderLocalVerts: Vec2Like[] | null;
  footprintAreaPx2: number;
  boundingWidthPx: number;
  boundingHeightPx: number;
  boundingAreaPx2: number;
}

export interface SpawnPlanItem {
  model: Model;
  inst: number;
  spawnX: number;
  metrics: ModelFootprintMetrics;
}

export interface Props {
  models: Model[];
  items: Record<string, number>;
  selectedPrinterId: string | null;
  printers: PrinterConfig[];
  bedWidthMm: number;
  bedDepthMm: number;
  onPrinterChange?: (printerId: string) => void;
  spawnOrder: SpawnType;
  hullMode: HullMode;
  onSpawnOrderChange?: (next: SpawnType) => void;
  onHullModeChange?: (next: HullMode) => void;
  onPausedChange?: (paused: boolean) => void;
}
