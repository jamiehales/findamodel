import { useEffect, useMemo, useRef, useState } from 'react';
import ConfirmDialog from '../ConfirmDialog';
import * as PIXI from 'pixi.js';
import { World, Vec2, Box } from 'planck';
import type { Body } from 'planck';
import {
  Stack,
  Typography,
  Button,
  MenuItem,
  Select,
  Checkbox,
  FormGroup,
  FormControlLabel,
  FormControl,
  InputLabel,
} from '@mui/material';
import type { SpawnType, HullMode } from '../../lib/api';
import {
  CANVAS_WIDTH_MM,
  CANVAS_HEIGHT_MM,
  CANVAS_WIDTH_PX,
  CANVAS_HEIGHT_PX,
  VIEW_TOP_MARGIN_PX,
  VIEW_HEIGHT_PX,
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
  CAT_WALL,
  CAT_BORDER,
  toPhysics,
  toPixels,
} from './constants';
import type { SavedLayout, Entry, Props } from './types';
import { getModelFootprintMetrics } from './hullHelpers';
import { buildSpawnPlan, getIncrementalSpawnX } from './spawnStrategy';
import { createModelBody, queryPointPx } from './physicsWorld';
import {
  computeOverlapping,
  isOutOfBounds,
  drawBody,
  drawDottedRect,
  darkenColor,
} from './renderer';

export { LAYOUT_LOCALSTORAGE_KEY };

const GRID_PX = 10 * PX_PER_MM;

const PAGE_H_PADDING = 128; // ~4rem per side at wide viewports
function getCanvasScale(): number {
  return Math.min(1.5, Math.max(1, (window.innerWidth - PAGE_H_PADDING) / CANVAS_WIDTH_PX));
}

export default function PrintingListCanvas({
  models,
  items,
  spawnOrder,
  hullMode,
  onSpawnOrderChange,
  onHullModeChange,
  onPausedChange,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const itemsKey = useMemo(() => JSON.stringify(items), [items]);
  const [resetCount, setResetCount] = useState(0);
  const [canvasScale, setCanvasScale] = useState(getCanvasScale);
  const canvasScaleRef = useRef(canvasScale);
  useEffect(() => {
    const handleResize = () => {
      const s = getCanvasScale();
      canvasScaleRef.current = s;
      setCanvasScale(s);
    };
    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  }, []);

  // Resize the live PIXI app when scale changes (no simulation restart)
  useEffect(() => {
    const app = appRef.current;
    if (!app) return;
    displayScaleRef.current = canvasScale;
    app.renderer.resize(CANVAS_WIDTH_PX * canvasScale, VIEW_HEIGHT_PX * canvasScale);
    app.stage.hitArea = new PIXI.Rectangle(
      0,
      0,
      CANVAS_WIDTH_PX * canvasScale,
      VIEW_HEIGHT_PX * canvasScale,
    );
    redrawStaticsRef.current?.(canvasScale);
  }, [canvasScale]);
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
      width: CANVAS_WIDTH_PX * initScale,
      height: VIEW_HEIGHT_PX * initScale,
      backgroundColor: 0x0f172a,
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
      CANVAS_WIDTH_PX * initScale,
      VIEW_HEIGHT_PX * initScale,
    );

    // ── planck.js world ────────────────────────────────────────────────────
    const world = new World(Vec2(0, 20));
    worldRef.current = world;

    // ── Walls (static, CAT_WALL — only collide with CAT_BORDER fixtures) ──
    const wallFixOpt = {
      filterCategoryBits: CAT_WALL,
      filterMaskBits: CAT_BORDER,
      friction: 0.5,
    };

    const ground = world.createBody({
      type: 'static' as const,
      position: Vec2(
        toPhysics(CANVAS_WIDTH_PX / 2),
        toPhysics(CANVAS_HEIGHT_PX + WALL_THICKNESS / 2 - PHYSICS_BORDER_PADDING_PX),
      ),
    });
    ground.createFixture(
      new Box(toPhysics((CANVAS_WIDTH_PX + WALL_THICKNESS * 2) / 2), toPhysics(WALL_THICKNESS / 2)),
      wallFixOpt,
    );

    const wallLeft = world.createBody({
      type: 'static' as const,
      position: Vec2(toPhysics(-WALL_THICKNESS / 2), toPhysics(CANVAS_HEIGHT_PX / 2)),
    });
    wallLeft.createFixture(
      new Box(
        toPhysics(WALL_THICKNESS / 2),
        toPhysics((CANVAS_HEIGHT_PX + WALL_THICKNESS * 2) / 2),
      ),
      wallFixOpt,
    );

    const wallRight = world.createBody({
      type: 'static' as const,
      position: Vec2(
        toPhysics(CANVAS_WIDTH_PX + WALL_THICKNESS / 2),
        toPhysics(CANVAS_HEIGHT_PX / 2),
      ),
    });
    wallRight.createFixture(
      new Box(
        toPhysics(WALL_THICKNESS / 2),
        toPhysics((CANVAS_HEIGHT_PX + WALL_THICKNESS * 2) / 2),
      ),
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
      gridGfx.lineStyle(0.5, 0x334155, 0.6);
      for (let x = scaledGridPx; x < CANVAS_WIDTH_PX * sc; x += scaledGridPx)
        gridGfx
          .moveTo(x, VIEW_TOP_MARGIN_PX * sc)
          .lineTo(x, (VIEW_TOP_MARGIN_PX + CANVAS_HEIGHT_PX) * sc);
      for (let y = scaledGridPx; y < CANVAS_HEIGHT_PX * sc; y += scaledGridPx)
        gridGfx
          .moveTo(0, VIEW_TOP_MARGIN_PX * sc + y)
          .lineTo(CANVAS_WIDTH_PX * sc, VIEW_TOP_MARGIN_PX * sc + y);

      borderGfx.clear();
      borderGfx.lineStyle({ width: 2, color: 0x64748b, alpha: 1, alignment: 0 });
      borderGfx.drawRect(0, VIEW_TOP_MARGIN_PX * sc, CANVAS_WIDTH_PX * sc, CANVAS_HEIGHT_PX * sc);

      const wasVisible = outOfBoundsBorderGfx.visible;
      outOfBoundsBorderGfx.clear();
      outOfBoundsBorderGfx.lineStyle({ width: 2, color: 0xef4444, alpha: 1, alignment: 0 });
      drawDottedRect(
        outOfBoundsBorderGfx,
        0,
        VIEW_TOP_MARGIN_PX * sc,
        CANVAS_WIDTH_PX * sc,
        CANVAS_HEIGHT_PX * sc,
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

    let spawnY = -40; // bodies spawn above the canvas and fall in

    const spawnPlan = buildSpawnPlan(models, items, spawnOrder, hullMode);

    for (const { model, inst, spawnX, metrics } of spawnPlan) {
      const color = modelColor.get(model.id) ?? PALETTE[0];

      const { body, visualLocalVerts, borderLocalVerts } = createModelBody(
        world,
        spawnX,
        spawnY,
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
        fontFamily: 'system-ui, -apple-system, sans-serif',
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
    // Only autosave on settle when there was no prior layout — with an existing
    // layout the user must explicitly save after adjusting positions.
    const shouldAutosave = savedLayout === null;
    pausedRef.current = savedLayout !== null;
    notifyLayoutCleanRef.current?.(savedLayout !== null);
    // Record which items are now in the simulation (for incremental updates)
    prevItemsRef.current = { ...items };
    let settleFrames = 0;
    let drag: { body: Body; ox: number; oy: number } | null = null;
    let prevHasOutOfBounds = false;

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
        const outOfBounds = isOutOfBounds(body, bVerts);
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
    const onWheel = (e: WheelEvent) => {
      if (!drag) return;
      e.preventDefault();
      drag.body.setAngle(drag.body.getAngle() + e.deltaY * 0.003);
    };
    canvas.addEventListener('wheel', onWheel, { passive: false });

    // ── Ticker ─────────────────────────────────────────────────────────────
    app.ticker.add(() => {
      if (!pausedRef.current) {
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
          if (settleFrames >= SETTLE_FRAMES) {
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
      redrawStaticsRef.current = null;
      app.destroy(true, { children: true, texture: true, baseTexture: true });
      // planck world is GC'd when ref is released — no explicit clear needed
    };
  }, [spawnOrder, hullMode, resetCount]);

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
          const spawnX = getIncrementalSpawnX(
            spawnOrder,
            model,
            currentModels,
            currItems,
            entriesRef.current,
            hullModeRef.current,
          );

          const { body, visualLocalVerts, borderLocalVerts } = createModelBody(
            world,
            spawnX,
            spawnY,
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
              fontFamily: 'system-ui, -apple-system, sans-serif',
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
      }
    }

    prevItemsRef.current = { ...currItems };
  }, [itemsKey, spawnOrder]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <Stack direction="column" spacing={1}>
      <Stack direction="row" spacing={1} alignContent={'center'} alignItems="center">
        <Typography variant="body2" color="text.secondary">
          Printer: Uniformation GK2
          <br />
          Print area: {CANVAS_WIDTH_MM} × {CANVAS_HEIGHT_MM} mm
        </Typography>
        <FormControl size="small">
          <InputLabel id="spawn-order-label">Spawn order</InputLabel>
          <Select
            labelId="spawn-order-label"
            label="Spawn order"
            value={spawnOrder}
            onChange={(e) => {
              const next = e.target.value as SpawnType;
              localStorage.removeItem(LAYOUT_LOCALSTORAGE_KEY);
              setHasSavedLayout(false);
              setUndoLayoutData(null);
              onSpawnOrderChange?.(next);
            }}
          >
            <MenuItem value="grouped">Grouped</MenuItem>
            <MenuItem value="random">Random</MenuItem>
            <MenuItem value="largestFirstFillGaps">Largest first, fill gaps</MenuItem>
          </Select>
        </FormControl>
        <FormControl size="small">
          <InputLabel id="hull-mode-label">Hull mode</InputLabel>
          <Select
            labelId="hull-mode-label"
            label="Hull mode"
            value={hullMode}
            onChange={(e) => {
              const next = e.target.value as HullMode;
              localStorage.removeItem(LAYOUT_LOCALSTORAGE_KEY);
              setHasSavedLayout(false);
              setUndoLayoutData(null);
              onHullModeChange?.(next);
            }}
          >
            <MenuItem value="convex">Convex hull</MenuItem>
            <MenuItem value="sansRaft">Sans raft hull</MenuItem>
          </Select>
        </FormControl>
        <FormGroup>
          <FormControlLabel
            label="Show labels"
            control={
              <Checkbox
                checked={showLabels}
                onChange={() => {
                  const next = !showLabels;
                  localStorage.setItem(SHOW_LABELS_LOCALSTORAGE_KEY, String(next));
                  setShowLabels(next);
                }}
              />
            }
          />
        </FormGroup>
        <FormGroup>
          <FormControlLabel
            label="Pause on drag"
            control={
              <Checkbox
                checked={pauseOnDrag}
                onChange={() => {
                  const next = !pauseOnDrag;
                  localStorage.setItem(PAUSE_ON_DRAG_LOCALSTORAGE_KEY, String(next));
                  setPauseOnDrag(next);
                }}
              />
            }
          />
        </FormGroup>
        <Stack direction="row" spacing={1} style={{ marginLeft: 'auto' }}>
          <Button
            size="large"
            variant="outlined"
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
            size="large"
            variant="outlined"
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
            size="large"
            variant={isLayoutClean ? 'outlined' : 'primary'}
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
            Save
          </Button>
          <ConfirmDialog
            open={warnSaveOpen}
            title="Models off canvas"
            message="Some models are outside the print area and will not be included in the exported plate. Save anyway?"
            confirmLabel="Save"
            onConfirm={() => {
              setWarnSaveOpen(false);
              pausedRef.current = true;
              saveLayoutRef.current?.();
            }}
            onCancel={() => setWarnSaveOpen(false)}
          />
        </Stack>
      </Stack>
      <div
        ref={containerRef}
        style={{
          width: CANVAS_WIDTH_PX * canvasScale,
          height: VIEW_HEIGHT_PX * canvasScale,
          borderRadius: 0,
          overflow: 'hidden',
          boxShadow: '0 4px 24px rgba(0,0,0,0.4)',
          cursor: 'default',
        }}
      />
    </Stack>
  );
}
