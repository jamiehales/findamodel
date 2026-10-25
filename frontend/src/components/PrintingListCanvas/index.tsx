import { useEffect, useMemo, useRef, useState } from 'react';
import ConfirmDialog from '../ConfirmDialog';
import * as PIXI from 'pixi.js';
import { World, Vec2, Box } from 'planck';
import type { Body } from 'planck';
import {
  Button,
  Checkbox,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@jamiehales/ui-kit/mostlymaking';
import styles from './PrintingListCanvas.module.css';
import type { SpawnType, HullMode } from '../../lib/api';
import {
  VIEW_TOP_MARGIN_PX,
  WALL_THICKNESS,
  PHYSICS_BORDER_PADDING_PX,
  PX_PER_MM,
  SPEED_THRESH,
  ANGULAR_THRESH,
  SETTLE_FRAMES,
  PALETTE,
  LAYOUT_LOCALSTORAGE_KEY,
  PAUSE_ON_DRAG_LOCALSTORAGE_KEY,
  SHOW_LABELS_LOCALSTORAGE_KEY,
  PUSH_TO_EDGES_LOCALSTORAGE_KEY,
  GRAVITY_ACCEL,
  EDGE_SPRING,
  EDGE_DAMPING,
  EDGE_MAX_ACCEL,
  PLATE_ENTRY_TOLERANCE_PX,
  CENTRE_PLACEMENT_JITTER,
  CAT_WALL,
  CAT_BORDER,
  CAT_OBJECT,
  toPhysics,
  toPixels,
} from './constants';
import type { SavedLayout, Entry, Props, Vec2Like } from './types';
import { getModelFootprintMetrics } from './hullHelpers';
import { buildSpawnPlan, centreSpawnPoint, getIncrementalSpawnX } from './spawnStrategy';
import { createModelBody, fixtureBounds, isSpotClear, queryPointPx } from './physicsWorld';
import { awayFromCentre, edgeTargets } from './edgeLayout';
import {
  computeOverlapping,
  isOutOfBounds,
  drawBody,
  drawDottedRect,
  darkenColor,
} from './renderer';

export { LAYOUT_LOCALSTORAGE_KEY };

const GRID_PX = 10 * PX_PER_MM;
const ROTATION_SNAP_DEGREES = 5;
const WHEEL_DELTA_PER_ROTATION_STEP = 120;

// The plate scales to fill the width of its panel, within these limits.
function getCanvasScale(availableWidthPx: number, canvasWidthPx: number): number {
  if (availableWidthPx <= 0) return 1;
  return Math.min(1.5, Math.max(0.4, availableWidthPx / canvasWidthPx));
}

export default function PrintingListCanvas({
  models,
  items,
  selectedPrinterId,
  printers,
  bedWidthMm,
  bedDepthMm,
  onPrinterChange,
  spawnOrder,
  hullMode,
  onSpawnOrderChange,
  onHullModeChange,
  onPausedChange,
}: Props) {
  const canvasWidthMm = bedWidthMm;
  const canvasHeightMm = bedDepthMm;
  const canvasWidthPx = canvasWidthMm * PX_PER_MM;
  const canvasHeightPx = canvasHeightMm * PX_PER_MM;
  const viewHeightPx = canvasHeightPx + VIEW_TOP_MARGIN_PX;
  const canvasWidthPhys = toPhysics(canvasWidthPx);
  const canvasHeightPhys = toPhysics(canvasHeightPx);

  const containerRef = useRef<HTMLDivElement>(null);
  const itemsKey = useMemo(() => JSON.stringify(items), [items]);
  const [resetCount, setResetCount] = useState(0);
  const stageRef = useRef<HTMLDivElement>(null);
  const [canvasScale, setCanvasScale] = useState(1);
  const canvasScaleRef = useRef(canvasScale);

  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const update = () => {
      const s = getCanvasScale(stage.clientWidth, canvasWidthPx);
      canvasScaleRef.current = s;
      setCanvasScale(s);
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(stage);
    return () => observer.disconnect();
  }, [canvasWidthPx]);

  // Resize the live PIXI app when scale changes (no simulation restart)
  useEffect(() => {
    const app = appRef.current;
    if (!app) return;
    displayScaleRef.current = canvasScale;
    app.renderer.resize(canvasWidthPx * canvasScale, viewHeightPx * canvasScale);
    app.stage.hitArea = new PIXI.Rectangle(
      0,
      0,
      canvasWidthPx * canvasScale,
      viewHeightPx * canvasScale,
    );
    redrawStaticsRef.current?.(canvasScale);
  }, [canvasScale, canvasWidthPx, viewHeightPx]);
  const [pauseOnDrag, setPauseOnDrag] = useState(
    () => localStorage.getItem(PAUSE_ON_DRAG_LOCALSTORAGE_KEY) === 'true',
  );
  const pauseOnDragRef = useRef(pauseOnDrag);
  useEffect(() => {
    pauseOnDragRef.current = pauseOnDrag;
  }, [pauseOnDrag]);

  const [showLabels, setShowLabels] = useState(
    () => localStorage.getItem(SHOW_LABELS_LOCALSTORAGE_KEY) !== 'false',
  );
  const showLabelsRef = useRef(showLabels);
  useEffect(() => {
    showLabelsRef.current = showLabels;
  }, [showLabels]);

  const [pushToEdges, setPushToEdges] = useState(
    () => localStorage.getItem(PUSH_TO_EDGES_LOCALSTORAGE_KEY) === 'true',
  );
  const pushToEdgesRef = useRef(pushToEdges);
  useEffect(() => {
    pushToEdgesRef.current = pushToEdges;
  }, [pushToEdges]);

  const [hasSavedLayout, setHasSavedLayout] = useState(
    () => localStorage.getItem(LAYOUT_LOCALSTORAGE_KEY) !== null,
  );
  const [isLayoutClean, setIsLayoutClean] = useState(
    () => localStorage.getItem(LAYOUT_LOCALSTORAGE_KEY) !== null,
  );
  const [undoLayoutData, setUndoLayoutData] = useState<string | null>(null);
  const notifySavedLayoutRef = useRef<((v: boolean) => void) | null>(null);
  notifySavedLayoutRef.current = setHasSavedLayout;
  // Drives isLayoutClean + the parent Export-button gate (onPausedChange)
  const notifyLayoutCleanRef = useRef<((v: boolean) => void) | null>(null);
  notifyLayoutCleanRef.current = (v) => {
    setIsLayoutClean(v);
    onPausedChange?.(v);
  };

  const [warnSaveOpen, setWarnSaveOpen] = useState(false);
  const hasOutOfBoundsRef = useRef(false);

  // Refs that let the incremental-update effect reach into the running simulation
  const appRef = useRef<PIXI.Application | null>(null);
  const worldRef = useRef<InstanceType<typeof World> | null>(null);
  const entriesRef = useRef<Entry[]>([]);
  const dynamicBodiesRef = useRef<Body[]>([]);
  const modelColorRef = useRef<Map<string, number>>(new Map());
  const pausedRef = useRef(false);
  const prevItemsRef = useRef<Record<string, number>>({});
  const saveLayoutRef = useRef<(() => void) | null>(null);
  const resumeSimulationRef = useRef<(() => void) | null>(null);
  const respreadEdgesRef = useRef<(() => void) | null>(null);
  const displayScaleRef = useRef(canvasScale);
  const redrawStaticsRef = useRef<((sc: number) => void) | null>(null);
  // Always-current mirrors updated every render
  const itemsKeyRef = useRef(itemsKey);
  itemsKeyRef.current = itemsKey;
  const modelsRef = useRef(models);
  modelsRef.current = models;
  const hullModeRef = useRef<HullMode>(hullMode);
  hullModeRef.current = hullMode;

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    // ── Pixi application ───────────────────────────────────────────────────
    const initScale = canvasScaleRef.current;
    const app = new PIXI.Application({
      width: canvasWidthPx * initScale,
      height: viewHeightPx * initScale,
      backgroundColor: 0x18171a,
      antialias: true,
      resolution: Math.min(window.devicePixelRatio || 1, 2),
      autoDensity: true,
    });
    appRef.current = app;
    container.appendChild(app.view as HTMLCanvasElement);
    app.stage.eventMode = 'static';
    app.stage.hitArea = new PIXI.Rectangle(
      0,
      0,
      canvasWidthPx * initScale,
      viewHeightPx * initScale,
    );

    // ── planck.js world ────────────────────────────────────────────────────
    const world = new World(Vec2(0, GRAVITY_ACCEL));
    worldRef.current = world;

    // ── Walls (static, CAT_WALL - only collide with CAT_BORDER fixtures) ──
    const wallFixOpt = {
      filterCategoryBits: CAT_WALL,
      filterMaskBits: CAT_BORDER,
      friction: 0.5,
    };

    const ground = world.createBody({
      type: 'static' as const,
      position: Vec2(
        toPhysics(canvasWidthPx / 2),
        toPhysics(canvasHeightPx + WALL_THICKNESS / 2 - PHYSICS_BORDER_PADDING_PX),
      ),
    });
    ground.createFixture(
      new Box(toPhysics((canvasWidthPx + WALL_THICKNESS * 2) / 2), toPhysics(WALL_THICKNESS / 2)),
      wallFixOpt,
    );

    const wallLeft = world.createBody({
      type: 'static' as const,
      position: Vec2(toPhysics(-WALL_THICKNESS / 2), toPhysics(canvasHeightPx / 2)),
    });
    wallLeft.createFixture(
      new Box(toPhysics(WALL_THICKNESS / 2), toPhysics((canvasHeightPx + WALL_THICKNESS * 2) / 2)),
      wallFixOpt,
    );

    const wallRight = world.createBody({
      type: 'static' as const,
      position: Vec2(toPhysics(canvasWidthPx + WALL_THICKNESS / 2), toPhysics(canvasHeightPx / 2)),
    });
    wallRight.createFixture(
      new Box(toPhysics(WALL_THICKNESS / 2), toPhysics((canvasHeightPx + WALL_THICKNESS * 2) / 2)),
      wallFixOpt,
    );

    // Top edge: only used when pushing to edges, and only for models already on the plate,
    // so models above it can still be brought on.
    const wallTop = world.createBody({
      type: 'static' as const,
      position: Vec2(toPhysics(canvasWidthPx / 2), toPhysics(-WALL_THICKNESS / 2)),
    });
    wallTop.createFixture(
      new Box(toPhysics((canvasWidthPx + WALL_THICKNESS * 2) / 2), toPhysics(WALL_THICKNESS / 2)),
      wallFixOpt,
    );

    // ── Background ─────────────────────────────────────────────────────────
    const gridGfx = new PIXI.Graphics();
    app.stage.addChild(gridGfx);
    const borderGfx = new PIXI.Graphics();
    app.stage.addChild(borderGfx);
    const outOfBoundsBorderGfx = new PIXI.Graphics();
    outOfBoundsBorderGfx.visible = false;
    app.stage.addChild(outOfBoundsBorderGfx);

    function redrawStatics(sc: number) {
      const scaledGridPx = GRID_PX * sc;
      gridGfx.clear();
      gridGfx.lineStyle(0.5, 0x2a282e, 1);
      for (let x = scaledGridPx; x < canvasWidthPx * sc; x += scaledGridPx)
        gridGfx
          .moveTo(x, VIEW_TOP_MARGIN_PX * sc)
          .lineTo(x, (VIEW_TOP_MARGIN_PX + canvasHeightPx) * sc);
      for (let y = scaledGridPx; y < canvasHeightPx * sc; y += scaledGridPx)
        gridGfx
          .moveTo(0, VIEW_TOP_MARGIN_PX * sc + y)
          .lineTo(canvasWidthPx * sc, VIEW_TOP_MARGIN_PX * sc + y);

      borderGfx.clear();
      borderGfx.lineStyle({ width: 2, color: 0x6c6874, alpha: 1, alignment: 0 });
      borderGfx.drawRect(0, VIEW_TOP_MARGIN_PX * sc, canvasWidthPx * sc, canvasHeightPx * sc);

      const wasVisible = outOfBoundsBorderGfx.visible;
      outOfBoundsBorderGfx.clear();
      outOfBoundsBorderGfx.lineStyle({ width: 2, color: 0xe2685c, alpha: 1, alignment: 0 });
      drawDottedRect(
        outOfBoundsBorderGfx,
        0,
        VIEW_TOP_MARGIN_PX * sc,
        canvasWidthPx * sc,
        canvasHeightPx * sc,
        12 * sc,
        8 * sc,
      );
      outOfBoundsBorderGfx.visible = wasVisible;
    }
    redrawStaticsRef.current = redrawStatics;
    redrawStatics(initScale);

    // ── Saved layout ───────────────────────────────────────────────────────
    let savedLayout: SavedLayout | null = null;
    try {
      const raw = localStorage.getItem(LAYOUT_LOCALSTORAGE_KEY);
      if (raw) {
        const parsed: SavedLayout = JSON.parse(raw);
        if (parsed.itemsKey === itemsKey) savedLayout = parsed;
      }
    } catch {
      /* ignore */
    }

    // ── Build bodies + graphics ────────────────────────────────────────────
    const entries: Entry[] = [];
    entriesRef.current = entries;
    const dynamicBodies: Body[] = [];
    dynamicBodiesRef.current = dynamicBodies;

    const modelColor = new Map<string, number>();
    modelColorRef.current = modelColor;
    models.forEach((m, i) => modelColor.set(m.id, PALETTE[i % PALETTE.length]));

    // Bodies spawn above the canvas and fall in, or all at the middle for "center".
    let spawnY = -40;
    const spawnAtCentre = spawnOrder === 'center';

    const spawnPlan = buildSpawnPlan(models, items, spawnOrder, hullMode, canvasWidthPx);

    for (const { model, inst, spawnX, metrics } of spawnPlan) {
      const color = modelColor.get(model.id) ?? PALETTE[0];
      const start = spawnAtCentre
        ? centreSpawnPoint(canvasWidthPx, canvasHeightPx)
        : { x: spawnX, y: spawnY };

      const { body, visualLocalVerts, borderLocalVerts } = createModelBody(
        world,
        start.x,
        start.y,
        metrics.localVerts,
        metrics.borderLocalVerts,
        model,
      );

      // If there's a saved position for this instance, move the body there
      const saved = savedLayout?.positions.find(
        (p) => p.modelId === model.id && p.instanceIndex === inst,
      );
      if (saved) {
        body.setPosition(Vec2(toPhysics(saved.xMm * PX_PER_MM), toPhysics(saved.yMm * PX_PER_MM)));
        body.setAngle(saved.angle);
        body.setLinearVelocity(Vec2(0, 0));
        body.setAngularVelocity(0);
      }

      dynamicBodies.push(body);

      // Spread spawn points above canvas so bodies don't all overlap
      spawnY -= 50 + Math.max(metrics.boundingHeightPx, 60);

      // Pixi graphics (drawn in world-space each frame)
      const gfx = new PIXI.Graphics();
      app.stage.addChild(gfx);

      // Label
      const labelStr = model.name.length > 14 ? model.name.slice(0, 12) + '…' : model.name;
      const label = new PIXI.Text(labelStr, {
        fontFamily: 'IBM Plex Sans, system-ui, sans-serif',
        fontSize: 12,
        fill: 0xffffff,
        align: 'center',
      });
      label.anchor.set(0.5);
      label.roundPixels = true;
      app.stage.addChild(label);

      entries.push({
        body,
        gfx,
        label,
        modelId: model.id,
        instanceIndex: inst,
        color,
        visualLocalVerts,
        borderLocalVerts,
      });
    }

    // ── Simulation state ───────────────────────────────────────────────────
    // Start paused if positions were restored from storage; user clicks to resume.
    // Only autosave on settle when there was no prior layout - with an existing
    // layout the user must explicitly save after adjusting positions.
    const shouldAutosave = savedLayout === null;
    pausedRef.current = savedLayout !== null;
    notifyLayoutCleanRef.current?.(savedLayout !== null);
    // Record which items are now in the simulation (for incremental updates)
    prevItemsRef.current = { ...items };
    let settleFrames = 0;
    let drag: { body: Body; ox: number; oy: number } | null = null;
    let prevHasOutOfBounds = false;

    resumeSimulationRef.current = () => {
      pausedRef.current = false;
      notifyLayoutCleanRef.current?.(false);
      settleFrames = 0;
      edgeSlots = null;
      placedByHand.clear();
      for (const b of dynamicBodies) b.setAwake(true);
    };
    respreadEdgesRef.current = () => {
      edgeSlots = null;
    };

    // ── Push to edges ──────────────────────────────────────────────────────
    // True once the body's border hull is within the top of the plate.
    function isOnPlate(body: Body): boolean {
      const entry = entries.find((e) => e.body === body);
      if (!entry) return true;
      const tolerance = toPhysics(PLATE_ENTRY_TOLERANCE_PX);
      return entry.borderLocalVerts.every(
        (v) => body.getWorldPoint(Vec2(toPhysics(v.x), toPhysics(v.y))).y >= -tolerance,
      );
    }

    // Each model on the plate has a slot on the plate edge (see edgeTargets; null when the
    // edges are full), reassigned when the setting is turned on, a model arrives on the
    // plate, or one is removed. Models jump straight to their slot. A model the user has
    // dragged stays where it was dropped: it has no slot and is never moved for them.
    let edgeSlots: Map<Body, Vec2Like | null> | null = null;
    const placedByHand = new Set<Body>();
    // Set once the layout first settles: the forces stop so models pressing on each other
    // can separate before the layout counts as settled.
    let edgeForcesRelaxed = false;

    world.on('pre-solve', (contact) => {
      const a = contact.getFixtureA().getBody();
      const b = contact.getFixtureB().getBody();
      if (a !== wallTop && b !== wallTop) return;
      if (!pushToEdgesRef.current || !isOnPlate(a === wallTop ? b : a)) contact.setEnabled(false);
    });

    function assignEdgeSlots(onPlate: Body[]) {
      const placed = onPlate.flatMap((body) => {
        const border = fixtureBounds(body, CAT_BORDER);
        const object = fixtureBounds(body, CAT_OBJECT) ?? border;
        return border && object ? [{ body, border, object }] : [];
      });
      const targets = edgeTargets(
        placed.map(({ border, object }) => ({
          centre: border.centre,
          half: border.half,
          spacingHalf: object.half,
        })),
        canvasWidthPhys,
        canvasHeightPhys,
      );
      edgeSlots = new Map(placed.map(({ body }, i) => [body, targets[i]]));
      edgeForcesRelaxed = false;
      placed.forEach(({ body, border }, i) => {
        const target = targets[i];
        if (!target) return;
        // The slot is for the footprint's centre, which can sit off the body's origin.
        const pos = body.getPosition();
        body.setPosition(
          Vec2(target.x + pos.x - border.centre.x, target.y + pos.y - border.centre.y),
        );
        body.setLinearVelocity(Vec2(0, 0));
        body.setAngularVelocity(0);
        body.setAwake(true);
      });
    }

    // Models on the plate are held in their edge slot; when the edges are full, the rest are
    // pushed gently outward to pack in behind them and keep the centre clear. Models
    // above the plate (new or dragged off) wait there and are placed one at a time near the
    // centre; falling in would leave them resting on the models along the top edge.
    function applyEdgeForces() {
      const centre = { x: canvasWidthPhys / 2, y: canvasHeightPhys / 2 };
      const needSlots = entries
        .map((e) => e.body)
        .filter((b) => b.isDynamic() && !placedByHand.has(b) && isOnPlate(b));
      if (!edgeSlots || needSlots.some((b) => !edgeSlots!.has(b))) assignEdgeSlots(needSlots);

      let placed = false;
      for (const { body } of entries) {
        if (!body.isDynamic() || placedByHand.has(body)) continue;
        if (!isOnPlate(body)) {
          body.setLinearVelocity(Vec2(0, 0));
          body.setAngularVelocity(0);
          if (placed) continue;
          const spot = {
            x: centre.x * (1 + (Math.random() - 0.5) * CENTRE_PLACEMENT_JITTER),
            y: centre.y * (1 + (Math.random() - 0.5) * CENTRE_PLACEMENT_JITTER),
          };
          if (isSpotClear(world, body, spot)) {
            body.setPosition(Vec2(spot.x, spot.y));
            body.setAwake(true);
            placed = true;
          }
          continue;
        }
        if (edgeForcesRelaxed) continue;
        const target = edgeSlots?.get(body);
        const bounds = fixtureBounds(body, CAT_BORDER);
        if (!bounds) continue;
        const v = body.getLinearVelocity();
        let ax: number;
        let ay: number;
        if (target) {
          ax = EDGE_SPRING * (target.x - bounds.centre.x) - EDGE_DAMPING * v.x;
          ay = EDGE_SPRING * (target.y - bounds.centre.y) - EDGE_DAMPING * v.y;
        } else {
          const dir = awayFromCentre(bounds.centre, canvasWidthPhys, canvasHeightPhys);
          ax = EDGE_MAX_ACCEL * dir.x - EDGE_DAMPING * v.x;
          ay = EDGE_MAX_ACCEL * dir.y - EDGE_DAMPING * v.y;
        }
        const accel = Math.hypot(ax, ay);
        if (accel > EDGE_MAX_ACCEL) {
          ax *= EDGE_MAX_ACCEL / accel;
          ay *= EDGE_MAX_ACCEL / accel;
        }
        const mass = body.getMass();
        body.applyForceToCenter(Vec2(ax * mass, ay * mass), true);
      }
    }

    // ── Render helper ──────────────────────────────────────────────────────
    function renderEntries(overlapping: Set<Body>) {
      const sc = displayScaleRef.current;
      let hasOutOfBounds = false;
      for (const {
        body,
        gfx,
        label,
        color,
        visualLocalVerts,
        borderLocalVerts: bVerts,
      } of entries) {
        const outOfBounds = isOutOfBounds(body, bVerts, canvasWidthPhys, canvasHeightPhys);
        const renderColor = overlapping.has(body) ? color : darkenColor(color, 0.45);
        drawBody(
          gfx,
          body,
          visualLocalVerts,
          bVerts,
          renderColor,
          outOfBounds,
          VIEW_TOP_MARGIN_PX * sc,
          sc,
        );
        const pos = body.getPosition();
        label.visible = showLabelsRef.current;
        label.position.set(toPixels(pos.x) * sc, toPixels(pos.y) * sc + VIEW_TOP_MARGIN_PX * sc);
        hasOutOfBounds ||= outOfBounds;
      }
      outOfBoundsBorderGfx.visible = hasOutOfBounds;
      if (hasOutOfBounds !== prevHasOutOfBounds) {
        prevHasOutOfBounds = hasOutOfBounds;
        hasOutOfBoundsRef.current = hasOutOfBounds;
      }
    }

    // ── Layout persistence ─────────────────────────────────────────────────
    function saveLayout() {
      const layout: SavedLayout = {
        itemsKey: itemsKeyRef.current,
        positions: entries.map((e) => {
          const pos = e.body.getPosition();
          return {
            modelId: e.modelId,
            instanceIndex: e.instanceIndex,
            xMm: toPixels(pos.x) / PX_PER_MM,
            yMm: toPixels(pos.y) / PX_PER_MM,
            angle: e.body.getAngle(),
          };
        }),
      };
      localStorage.setItem(LAYOUT_LOCALSTORAGE_KEY, JSON.stringify(layout));
      notifySavedLayoutRef.current?.(true);
      notifyLayoutCleanRef.current?.(true);
    }
    saveLayoutRef.current = saveLayout;

    // ── Drag interaction ───────────────────────────────────────────────────
    app.stage.on('pointerdown', (e: PIXI.FederatedPointerEvent) => {
      // Resume simulation on any click when paused
      if (pausedRef.current) {
        pausedRef.current = false;
        notifyLayoutCleanRef.current?.(false);
        settleFrames = 0;
      }

      const x = e.global.x / canvasScaleRef.current;
      const y = e.global.y / canvasScaleRef.current;
      const worldY = y - VIEW_TOP_MARGIN_PX;
      const hit = queryPointPx(world, x, worldY);
      if (hit) {
        const pos = hit.getPosition();
        drag = { body: hit, ox: x - toPixels(pos.x), oy: worldY - toPixels(pos.y) };
        placedByHand.add(hit);
        hit.setType('kinematic');
        hit.setLinearVelocity(Vec2(0, 0));
        hit.setAngularVelocity(0);
        if (pauseOnDragRef.current) {
          pausedRef.current = true;
        }
      }
    });

    app.stage.on('pointermove', (e: PIXI.FederatedPointerEvent) => {
      if (!drag) return;
      const x = e.global.x / canvasScaleRef.current;
      const y = e.global.y / canvasScaleRef.current;
      const worldY = y - VIEW_TOP_MARGIN_PX;
      drag.body.setPosition(Vec2(toPhysics(x - drag.ox), toPhysics(worldY - drag.oy)));
    });

    const endDrag = () => {
      if (!drag) return;
      drag.body.setType('dynamic');
      drag.body.setLinearVelocity(Vec2(0, 0));
      drag.body.setAngularVelocity(0);
      drag.body.setAwake(true);
      drag = null;
      pausedRef.current = false;
      settleFrames = 0;
    };
    app.stage.on('pointerup', endDrag);
    app.stage.on('pointerupoutside', endDrag);

    const canvas = app.view as HTMLCanvasElement;
    let wheelDeltaAccumulator = 0;
    const onWheel = (e: WheelEvent) => {
      if (!drag) return;
      e.preventDefault();
      wheelDeltaAccumulator += e.deltaY;

      const steps = Math.trunc(Math.abs(wheelDeltaAccumulator) / WHEEL_DELTA_PER_ROTATION_STEP);
      if (steps === 0) return;

      const direction = Math.sign(wheelDeltaAccumulator);
      wheelDeltaAccumulator -= direction * steps * WHEEL_DELTA_PER_ROTATION_STEP;

      const currentDeg = (drag.body.getAngle() * 180) / Math.PI;
      const snappedCurrentDeg =
        Math.round(currentDeg / ROTATION_SNAP_DEGREES) * ROTATION_SNAP_DEGREES;
      const nextDeg = snappedCurrentDeg + direction * steps * ROTATION_SNAP_DEGREES;
      drag.body.setAngle((nextDeg * Math.PI) / 180);
    };
    canvas.addEventListener('wheel', onWheel, { passive: false });

    // ── Ticker ─────────────────────────────────────────────────────────────
    app.ticker.add(() => {
      if (!pausedRef.current) {
        const push = pushToEdgesRef.current;
        world.setGravity(Vec2(0, push ? 0 : GRAVITY_ACCEL));
        if (push) applyEdgeForces();
        // Cap delta to avoid physics explosion after tab becomes active again
        const deltaSec = Math.min(app.ticker.deltaMS, 50) / 1000;
        world.step(deltaSec, 8, 3);
      }

      renderEntries(computeOverlapping(entries));

      // Settling detection (skip when dragging or already paused)
      if (!pausedRef.current && !drag && dynamicBodies.length > 0) {
        const allSlow = dynamicBodies.every((b) => {
          const vel = b.getLinearVelocity();
          const speed = Math.sqrt(vel.x * vel.x + vel.y * vel.y);
          return speed < SPEED_THRESH && Math.abs(b.getAngularVelocity()) < ANGULAR_THRESH;
        });
        if (allSlow) {
          settleFrames++;
          if (settleFrames >= SETTLE_FRAMES && pushToEdgesRef.current && !edgeForcesRelaxed) {
            // The forces stop, so models pressed together separate before the layout counts
            // as settled.
            edgeForcesRelaxed = true;
            settleFrames = 0;
          } else if (settleFrames >= SETTLE_FRAMES) {
            pausedRef.current = true;
            if (shouldAutosave) {
              saveLayout();
            }
          }
        } else {
          settleFrames = 0;
        }
      }
    });

    // ── Cleanup ────────────────────────────────────────────────────────────
    return () => {
      canvas.removeEventListener('wheel', onWheel);
      appRef.current = null;
      worldRef.current = null;
      entriesRef.current = [];
      dynamicBodiesRef.current = [];
      modelColorRef.current = new Map();
      saveLayoutRef.current = null;
      resumeSimulationRef.current = null;
      respreadEdgesRef.current = null;
      redrawStaticsRef.current = null;
      app.destroy(true, { children: true, texture: true, baseTexture: true });
      // planck world is GC'd when ref is released - no explicit clear needed
    };
  }, [
    spawnOrder,
    hullMode,
    resetCount,
    canvasWidthPx,
    canvasHeightPx,
    canvasWidthPhys,
    canvasHeightPhys,
    viewHeightPx,
  ]);

  // ── Incremental effect: add/remove bodies when item counts change ──────────
  useEffect(() => {
    const app = appRef.current;
    const world = worldRef.current;
    if (!app || !world) return; // simulation not yet initialised

    const prevItems = prevItemsRef.current;
    const currItems = items;
    const currentModels = modelsRef.current;

    const allModelIds = new Set([...Object.keys(prevItems), ...Object.keys(currItems)]);

    for (const modelId of allModelIds) {
      const prevQty = prevItems[modelId] ?? 0;
      const currQty = currItems[modelId] ?? 0;
      if (prevQty === currQty) continue;

      const model = currentModels.find((m) => m.id === modelId);
      if (!model) continue;

      if (currQty > prevQty) {
        // ── Add new instances (from prevQty up to currQty-1) ───────────────
        // Assign a palette colour if this model hasn't appeared before
        if (!modelColorRef.current.has(modelId)) {
          modelColorRef.current.set(modelId, PALETTE[modelColorRef.current.size % PALETTE.length]);
        }
        let spawnY = -80;
        for (let inst = prevQty; inst < currQty; inst++) {
          const color = modelColorRef.current.get(modelId)!;
          const metrics = getModelFootprintMetrics(model, hullModeRef.current);
          const start =
            spawnOrder === 'center'
              ? centreSpawnPoint(canvasWidthPx, canvasHeightPx)
              : {
                  x: getIncrementalSpawnX(
                    spawnOrder,
                    model,
                    currentModels,
                    currItems,
                    entriesRef.current,
                    hullModeRef.current,
                    canvasWidthPx,
                  ),
                  y: spawnY,
                };

          const { body, visualLocalVerts, borderLocalVerts } = createModelBody(
            world,
            start.x,
            start.y,
            metrics.localVerts,
            metrics.borderLocalVerts,
            model,
          );

          spawnY -= 50 + Math.max(metrics.boundingHeightPx, 60);

          dynamicBodiesRef.current.push(body);

          const gfx = new PIXI.Graphics();
          app.stage.addChild(gfx);

          const label = new PIXI.Text(
            model.name.length > 14 ? model.name.slice(0, 12) + '…' : model.name,
            {
              fontFamily: 'IBM Plex Sans, system-ui, sans-serif',
              fontSize: 12,
              fill: 0xffffff,
              align: 'center',
            },
          );
          label.anchor.set(0.5);
          label.roundPixels = true;
          app.stage.addChild(label);

          entriesRef.current.push({
            body,
            gfx,
            label,
            modelId,
            instanceIndex: inst,
            color,
            visualLocalVerts,
            borderLocalVerts,
          });
        }

        pausedRef.current = false; // let new bodies fall in
        notifyLayoutCleanRef.current?.(false);
      } else {
        // Remove the models at the top of the build plate first
        for (let i = prevQty; i > currQty; i--) {
          let minY = Infinity;
          let toRemove: Entry | null = null;
          for (const e of entriesRef.current) {
            if (e.modelId === modelId && toPixels(e.body.getPosition().y) < minY) {
              minY = toPixels(e.body.getPosition().y);
              toRemove = e;
            }
          }
          if (!toRemove) break;

          world.destroyBody(toRemove.body);
          const dynIdx = dynamicBodiesRef.current.indexOf(toRemove.body);
          if (dynIdx >= 0) dynamicBodiesRef.current.splice(dynIdx, 1);

          app.stage.removeChild(toRemove.gfx);
          toRemove.gfx.destroy();
          app.stage.removeChild(toRemove.label);
          toRemove.label.destroy();

          const eIdx = entriesRef.current.indexOf(toRemove);
          if (eIdx >= 0) entriesRef.current.splice(eIdx, 1);
        }
        if (pushToEdgesRef.current) {
          // Re-spread the remaining models around the edges.
          respreadEdgesRef.current?.();
          pausedRef.current = false;
          notifyLayoutCleanRef.current?.(false);
        }
      }
    }

    prevItemsRef.current = { ...currItems };
  }, [itemsKey, spawnOrder, canvasWidthPx, canvasHeightPx]); // eslint-disable-line react-hooks/exhaustive-deps

  const resetSavedLayout = () => {
    localStorage.removeItem(LAYOUT_LOCALSTORAGE_KEY);
    setHasSavedLayout(false);
    setUndoLayoutData(null);
  };

  return (
    <section className={styles.plate} aria-label="Build plate">
      <div className={styles.bar}>
        <Select value={selectedPrinterId ?? undefined} onValueChange={(v) => onPrinterChange?.(v)}>
          <SelectTrigger aria-label="Printer" className={styles.printer}>
            <SelectValue placeholder="Printer">
              {printers.find((p) => p.id === selectedPrinterId)?.name}
            </SelectValue>
          </SelectTrigger>
          <SelectContent>
            {printers.map((printer) => (
              <SelectItem key={printer.id} value={printer.id}>
                {printer.name} ({printer.bedWidthMm} × {printer.bedDepthMm} mm)
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select
          value={spawnOrder}
          onValueChange={(v) => {
            resetSavedLayout();
            onSpawnOrderChange?.(v as SpawnType);
          }}
        >
          <SelectTrigger aria-label="Spawn order" className={styles.small}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="grouped">Grouped</SelectItem>
            <SelectItem value="random">Random</SelectItem>
            <SelectItem value="largestFirstFillGaps">Largest first</SelectItem>
            <SelectItem value="center">Center</SelectItem>
          </SelectContent>
        </Select>
        <Select
          value={hullMode}
          onValueChange={(v) => {
            resetSavedLayout();
            onHullModeChange?.(v as HullMode);
          }}
        >
          <SelectTrigger aria-label="Hull mode" className={styles.small}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="convex">Convex hull</SelectItem>
            <SelectItem value="sansRaft">Without raft</SelectItem>
          </SelectContent>
        </Select>
        <Checkbox
          checked={showLabels}
          onCheckedChange={() => {
            const next = !showLabels;
            localStorage.setItem(SHOW_LABELS_LOCALSTORAGE_KEY, String(next));
            setShowLabels(next);
          }}
        >
          Labels
        </Checkbox>
        <Checkbox
          checked={pauseOnDrag}
          onCheckedChange={() => {
            const next = !pauseOnDrag;
            localStorage.setItem(PAUSE_ON_DRAG_LOCALSTORAGE_KEY, String(next));
            setPauseOnDrag(next);
          }}
        >
          Pause on drag
        </Checkbox>
        <Checkbox
          checked={pushToEdges}
          onCheckedChange={() => {
            const next = !pushToEdges;
            localStorage.setItem(PUSH_TO_EDGES_LOCALSTORAGE_KEY, String(next));
            pushToEdgesRef.current = next;
            setPushToEdges(next);
            resumeSimulationRef.current?.();
          }}
        >
          Push to edges
        </Checkbox>
        <div className={styles.spacer} />
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            if (undoLayoutData !== null) {
              localStorage.setItem(LAYOUT_LOCALSTORAGE_KEY, undoLayoutData);
              setHasSavedLayout(true);
              setIsLayoutClean(true);
              setUndoLayoutData(null);
            }
            setResetCount((c) => c + 1);
          }}
          disabled={undoLayoutData === null && (isLayoutClean || !hasSavedLayout)}
        >
          Undo
        </Button>
        <Button
          size="sm"
          onClick={() => {
            const existing = localStorage.getItem(LAYOUT_LOCALSTORAGE_KEY);
            if (existing) setUndoLayoutData(existing);
            localStorage.removeItem(LAYOUT_LOCALSTORAGE_KEY);
            setHasSavedLayout(false);
            setIsLayoutClean(false);
            setResetCount((c) => c + 1);
          }}
        >
          Reset
        </Button>
        <Button
          size="sm"
          variant="secondary"
          onClick={() => {
            if (hasOutOfBoundsRef.current) {
              setWarnSaveOpen(true);
            } else {
              pausedRef.current = true;
              saveLayoutRef.current?.();
            }
          }}
          disabled={isLayoutClean}
        >
          Save layout
        </Button>
        <ConfirmDialog
          open={warnSaveOpen}
          title="Save with models off the plate?"
          message="Models outside the print area won't be included in the exported plate."
          confirmLabel="Save"
          destructive={false}
          onConfirm={() => {
            setWarnSaveOpen(false);
            pausedRef.current = true;
            saveLayoutRef.current?.();
          }}
          onCancel={() => setWarnSaveOpen(false)}
        />
      </div>
      <div ref={stageRef} className={styles.stage}>
        <div
          ref={containerRef}
          className={styles.canvas}
          style={{ width: canvasWidthPx * canvasScale, height: viewHeightPx * canvasScale }}
        />
      </div>
    </section>
  );
}
