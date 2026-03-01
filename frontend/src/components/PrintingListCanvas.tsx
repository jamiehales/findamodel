import { useEffect, useMemo, useRef, useState } from 'react'
import * as PIXI from 'pixi.js'
import Matter from 'matter-js'
import { Stack, Typography, Button, MenuItem, Select, ToggleButton, ToggleButtonGroup, Checkbox, FormGroup, FormControlLabel } from '@mui/material'
import type { Model } from '../lib/api'

// ── Constants ─────────────────────────────────────────────────────────────────

const CANVAS_WIDTH_MM = 228
const CANVAS_HEIGHT_MM = 128
const PX_PER_MM = 4
const CANVAS_WIDTH_PX = CANVAS_WIDTH_MM * PX_PER_MM
const CANVAS_HEIGHT_PX = CANVAS_HEIGHT_MM * PX_PER_MM

const WALL_THICKNESS = 200
const SPEED_THRESH = 0.12
const ANGULAR_THRESH = 0.008
const SETTLE_FRAMES = 90 // ~1.5 s at 60 fps
export const LAYOUT_LOCALSTORAGE_KEY = 'findamodel.printingListLayout'
const SPAWN_ORDER_LOCALSTORAGE_KEY = 'findamodel.printingListSpawnOrder'
const PAUSE_ON_DRAG_LOCALSTORAGE_KEY = 'findamodel.printingListPauseOnDrag'
const HULL_MODE_LOCALSTORAGE_KEY = 'findamodel.printingListHullMode'
const DEBUG_PHYSICS_WIREFRAME = false

// Physics body inflation — when two bodies touch they have BODY_GAP_MM visual gap.
const BODY_GAP_MM = 2
const BODY_MARGIN_PX = (BODY_GAP_MM / 2) * PX_PER_MM

const PALETTE = [
  0x818cf8, // indigo
  0x34d399, // emerald
  0xfb923c, // orange
  0xf472b6, // pink
  0xa78bfa, // violet
  0x38bdf8, // sky
  0xfbbf24, // amber
  0xf87171, // rose
]

// ── Types ─────────────────────────────────────────────────────────────────────

interface Vec2 {
  x: number
  y: number
}

interface SavedLayout {
  itemsKey: string
  positions: {
    modelId: string
    instanceIndex: number
    xMm: number
    yMm: number
    angle: number
  }[]
}

interface Entry {
  body: Matter.Body
  gfx: PIXI.Graphics
  label: PIXI.Text
  modelId: string
  instanceIndex: number
  color: number
  /** Original (non-inflated) hull vertices in body-local space, for rendering. */
  visualLocalVerts: Vec2[]
}

interface Props {
  models: Model[]
  items: Record<string, number>
  onPausedChange?: (paused: boolean) => void
}

// ── Hull helpers ──────────────────────────────────────────────────────────────

/**
 * Parse a JSON hull string ("[[x,z],[x,z],...]") into local-space pixel
 * vertices centred at the origin
 */
function parseHullLocalPx(hullJson: string | null): Vec2[] | null {
  if (!hullJson) return null
  try {
    const raw: [number, number][] = JSON.parse(hullJson)
    if (raw.length < 3) return null

    // Hull coords are [x, z] in model space (assumed mm for 3-D printing STLs).
    // Map z → canvas y.
    const pts = raw.map(([x, z]): Vec2 => ({ x: x * PX_PER_MM, y: z * PX_PER_MM }))

    // Centre at the polygon area centroid (shoelace formula) so it matches
    // what Matter.js uses internally, minimising the shift fromVertices applies.
    let area = 0
    let cx = 0
    let cy = 0
    for (let i = 0, n = pts.length; i < n; i++) {
      const a = pts[i], b = pts[(i + 1) % n]
      const cross = a.x * b.y - b.x * a.y
      area += cross
      cx += (a.x + b.x) * cross
      cy += (a.y + b.y) * cross
    }
    area /= 2
    cx /= 6 * area
    cy /= 6 * area
    const centred = pts.map(p => ({ x: p.x - cx, y: p.y - cy }))

    return centred
  } catch {
    return null
  }
}

// ── Geometry helpers ──────────────────────────────────────────────────────────

/**
 * Expand each vertex outward by `amount` px along its vertex normal.
 * The vertex normal is the average of the outward-facing normals of the two
 * adjacent edges, making this a true parallel offset for convex hulls.
 * Winding-order agnostic: the result is flipped to face away from origin when
 * needed (verts must be centred at origin, as parseHullLocalPx guarantees).
 */
function inflateVerts(verts: Vec2[], amount: number): Vec2[] {
  const n = verts.length
  return verts.map((v, i) => {
    const prev = verts[(i - 1 + n) % n]
    const next = verts[(i + 1) % n]

    // Perpendicular (rotated 90° CW) to each adjacent edge.
    const nx = (v.y - prev.y) + (next.y - v.y)
    const ny = -(v.x - prev.x) + -(next.x - v.x)

    const len = Math.sqrt(nx * nx + ny * ny)
    if (len < 0.001) return v

    // Normalise; flip if pointing toward origin rather than away from it.
    const sign = nx * v.x + ny * v.y < 0 ? -1 : 1
    return { x: v.x + (sign * nx / len) * amount, y: v.y + (sign * ny / len) * amount }
  })
}

// ── Body factory helpers ──────────────────────────────────────────────────────

const BODY_MASS = 0.01

const BODY_OPTIONS: Matter.IChamferableBodyDefinition = {
  restitution: 0.05,
  friction: 0.5,
  frictionAir: 0.025,
}

function makePolygonBody(cx: number, cy: number, localVerts: Vec2[]): Matter.Body {
  // fromVertices centres the body at (cx, cy) using the centroid of the passed
  // vertices. Since localVerts are already centred at origin, their centroid ≈ 0,
  // and Matter.js will correctly place the body at (cx, cy).
  const body = Matter.Bodies.fromVertices(0, 0, [localVerts as Matter.Vector[]], BODY_OPTIONS)
  Matter.Body.setMass(body, BODY_MASS)
  Matter.Body.setPosition(body, { x: cx, y: cy })
  return body
}

/**
 * Returns the physics body (inflated by BODY_MARGIN_PX per side), the visual
 * local vertices (original size), and the overlap-detection vertices (inflated
 * by BODY_OVERLAP_PX per side).
 */
function makeRectBody(
  model: Model,
  cx: number,
  cy: number,
): { body: Matter.Body; visualLocalVerts: Vec2[] } {
  // TODO: There is a minimum body size set here, check what we want to happen if this condition is met
  const w = Math.max((model.dimensionXMm ?? 20) * PX_PER_MM, 16)
  const h = Math.max((model.dimensionZMm ?? 20) * PX_PER_MM, 16)
  const body = Matter.Bodies.rectangle(
    cx,
    cy,
    w + BODY_MARGIN_PX * 2,
    h + BODY_MARGIN_PX * 2,
    BODY_OPTIONS,
  )
  const hw = w / 2
  const hh = h / 2
  const visualLocalVerts: Vec2[] = [
    { x: -hw, y: -hh },
    { x: hw, y: -hh },
    { x: hw, y: hh },
    { x: -hw, y: hh },
  ]
  return { body, visualLocalVerts }
}

// ── Color helpers ─────────────────────────────────────────────────────────────

function darkenColor(color: number, factor: number): number {
  const r = Math.round(((color >> 16) & 0xff) * factor)
  const g = Math.round(((color >> 8) & 0xff) * factor)
  const b = Math.round((color & 0xff) * factor)
  return (r << 16) | (g << 8) | b
}

// ── Overlap helpers ───────────────────────────────────────────────────────────

/** Transform local-space vertices to world space using a body's pose. */
function toWorldVerts(body: Matter.Body, localVerts: Vec2[]): Vec2[] {
  const cos = Math.cos(body.angle)
  const sin = Math.sin(body.angle)
  const { x: px, y: py } = body.position
  return localVerts.map(v => ({
    x: v.x * cos - v.y * sin + px,
    y: v.x * sin + v.y * cos + py,
  }))
}

/**
 * Separating Axis Theorem overlap test for two convex polygons.
 * Returns true if they overlap (no separating axis found).
 */
function satOverlap(a: Vec2[], b: Vec2[]): boolean {
  for (const poly of [a, b]) {
    const n = poly.length
    for (let i = 0; i < n; i++) {
      const p1 = poly[i]
      const p2 = poly[(i + 1) % n]
      const nx = -(p2.y - p1.y)
      const ny = p2.x - p1.x
      let minA = Infinity, maxA = -Infinity
      let minB = Infinity, maxB = -Infinity
      for (const v of a) { const d = v.x * nx + v.y * ny; minA = Math.min(minA, d); maxA = Math.max(maxA, d) }
      for (const v of b) { const d = v.x * nx + v.y * ny; minB = Math.min(minB, d); maxB = Math.max(maxB, d) }
      if (maxA < minB || maxB < minA) return false
    }
  }
  return true
}

/** Returns the set of body IDs whose overlap-detection hulls intersect any other entry. */
function computeOverlapping(entries: Entry[]): Set<number> {
  const overlapping = new Set<number>()
  const worldVerts = entries.map(e => toWorldVerts(e.body, e.visualLocalVerts))
  for (let i = 0; i < entries.length; i++) {
    for (let j = i + 1; j < entries.length; j++) {
      if (satOverlap(worldVerts[i], worldVerts[j])) {
        overlapping.add(entries[i].body.id)
        overlapping.add(entries[j].body.id)
      }
    }
  }
  return overlapping
}

// ── Draw helper ───────────────────────────────────────────────────────────────

/**
 * Draw the visual (non-inflated) hull by transforming local vertices through
 * the body's current position and angle. This keeps the rendered polygon at the
 * original model size while the physics body carries the 1 mm gap margin.
 */
function drawBody(gfx: PIXI.Graphics, body: Matter.Body, visualLocalVerts: Vec2[], color: number) {
  gfx.clear()
  if (!visualLocalVerts.length) return

  const cos = Math.cos(body.angle)
  const sin = Math.sin(body.angle)
  const { x: px, y: py } = body.position

  gfx.beginFill(color, 0.45)
  gfx.lineStyle(1.5, color, 0.9)
  const v0 = visualLocalVerts[0]
  gfx.moveTo(v0.x * cos - v0.y * sin + px, v0.x * sin + v0.y * cos + py)
  for (let i = 1; i < visualLocalVerts.length; i++) {
    const v = visualLocalVerts[i]
    gfx.lineTo(v.x * cos - v.y * sin + px, v.x * sin + v.y * cos + py)
  }
  gfx.closePath()
  gfx.endFill()

  if (DEBUG_PHYSICS_WIREFRAME) {
    gfx.lineStyle(1, 0xff0000, 0.5)
    gfx.moveTo(body.vertices[0].x, body.vertices[0].y)
    for (let i = 1; i < body.vertices.length; i++)
      gfx.lineTo(body.vertices[i].x, body.vertices[i].y)
    gfx.closePath()
  }
}

// ── Component ─────────────────────────────────────────────────────────────────

type SpawnOrder = 'grouped' | 'random'
type HullMode = 'convex' | 'sansRaft'

export default function PrintingListCanvas({ models, items, onPausedChange }: Props) {
  const containerRef = useRef<HTMLDivElement>(null)
  const itemsKey = useMemo(() => JSON.stringify(items), [items])
  const [spawnOrder, setSpawnOrder] = useState<SpawnOrder>(() => {
    const saved = localStorage.getItem(SPAWN_ORDER_LOCALSTORAGE_KEY)
    return saved === 'random' ? 'random' : 'grouped'
  })
  const [resetCount, setResetCount] = useState(0)
  const [hullMode, setHullMode] = useState<HullMode>(() => {
    const saved = localStorage.getItem(HULL_MODE_LOCALSTORAGE_KEY)
    return saved === 'sansRaft' ? 'sansRaft' : 'convex'
  })
  const [pauseOnDrag, setPauseOnDrag] = useState(
    () => localStorage.getItem(PAUSE_ON_DRAG_LOCALSTORAGE_KEY) === 'true',
  )
  const pauseOnDragRef = useRef(pauseOnDrag)
  useEffect(() => { pauseOnDragRef.current = pauseOnDrag }, [pauseOnDrag])

  const [isPaused, setIsPaused] = useState(false)

  // Refs that let the incremental-update effect reach into the running simulation
  const appRef = useRef<PIXI.Application | null>(null)
  const engineRef = useRef<Matter.Engine | null>(null)
  const entriesRef = useRef<Entry[]>([])
  const dynamicBodiesRef = useRef<Matter.Body[]>([])
  const modelColorRef = useRef<Map<string, number>>(new Map())
  const pausedRef = useRef(false)
  const prevItemsRef = useRef<Record<string, number>>({})
  // Lets the effect's ticker/handlers sync paused state back to React without stale closures
  const notifyPausedRef = useRef<((v: boolean) => void) | null>(null)
  notifyPausedRef.current = (v) => { setIsPaused(v); onPausedChange?.(v) }
  const saveLayoutRef = useRef<(() => void) | null>(null)
  // Always-current mirrors updated every render
  const itemsKeyRef = useRef(itemsKey)
  itemsKeyRef.current = itemsKey
  const modelsRef = useRef(models)
  modelsRef.current = models
  const hullModeRef = useRef(hullMode)
  hullModeRef.current = hullMode

  useEffect(() => {
    const container = containerRef.current
    if (!container) return

    // ── Pixi application ───────────────────────────────────────────────────
    const app = new PIXI.Application({
      width: CANVAS_WIDTH_PX,
      height: CANVAS_HEIGHT_PX,
      backgroundColor: 0x0f172a,
      antialias: true,
      resolution: Math.min(window.devicePixelRatio || 1, 2),
      autoDensity: true,
    })
    appRef.current = app
    container.appendChild(app.view as HTMLCanvasElement)
    app.stage.eventMode = 'static'
    app.stage.hitArea = new PIXI.Rectangle(0, 0, CANVAS_WIDTH_PX, CANVAS_HEIGHT_PX)

    // ── Matter.js engine ───────────────────────────────────────────────────
    const engine = Matter.Engine.create({ gravity: { x: 0, y: 1 } })
    engineRef.current = engine

    const ground = Matter.Bodies.rectangle(
      CANVAS_WIDTH_PX / 2,
      CANVAS_HEIGHT_PX + WALL_THICKNESS / 2,
      CANVAS_WIDTH_PX + WALL_THICKNESS * 2,
      WALL_THICKNESS,
      { isStatic: true },
    )
    const wallLeft = Matter.Bodies.rectangle(
      -WALL_THICKNESS / 2,
      CANVAS_HEIGHT_PX / 2,
      WALL_THICKNESS,
      CANVAS_HEIGHT_PX + WALL_THICKNESS * 2,
      { isStatic: true },
    )
    const wallRight = Matter.Bodies.rectangle(
      CANVAS_WIDTH_PX + WALL_THICKNESS / 2,
      CANVAS_HEIGHT_PX / 2,
      WALL_THICKNESS,
      CANVAS_HEIGHT_PX + WALL_THICKNESS * 2,
      { isStatic: true },
    )
    Matter.Composite.add(engine.world, [ground, wallLeft, wallRight])

    // ── Background ─────────────────────────────────────────────────────────
    const GRID_PX = 10 * PX_PER_MM
    const gridGfx = new PIXI.Graphics()
    gridGfx.lineStyle(0.5, 0x334155, 0.6)
    for (let x = GRID_PX; x < CANVAS_WIDTH_PX; x += GRID_PX) gridGfx.moveTo(x, 0).lineTo(x, CANVAS_HEIGHT_PX)
    for (let y = GRID_PX; y < CANVAS_HEIGHT_PX; y += GRID_PX) gridGfx.moveTo(0, y).lineTo(CANVAS_WIDTH_PX, y)
    app.stage.addChild(gridGfx)

    const borderGfx = new PIXI.Graphics()
    borderGfx.lineStyle(2, 0x64748b, 1)
    borderGfx.drawRect(0, 0, CANVAS_WIDTH_PX, CANVAS_HEIGHT_PX)
    app.stage.addChild(borderGfx)

    // ── Saved layout ───────────────────────────────────────────────────────
    let savedLayout: SavedLayout | null = null
    try {
      const raw = localStorage.getItem(LAYOUT_LOCALSTORAGE_KEY)
      if (raw) {
        const parsed: SavedLayout = JSON.parse(raw)
        if (parsed.itemsKey === itemsKey) savedLayout = parsed
      }
    } catch { /* ignore */ }

    // ── Build bodies + graphics ────────────────────────────────────────────
    const entries: Entry[] = []
    entriesRef.current = entries
    const dynamicBodies: Matter.Body[] = []
    dynamicBodiesRef.current = dynamicBodies

    const modelColor = new Map<string, number>()
    modelColorRef.current = modelColor
    models.forEach((m, i) => modelColor.set(m.id, PALETTE[i % PALETTE.length]))

    let spawnY = -40 // bodies spawn above the canvas and fall in

    // Build the flat spawn sequence, then reorder based on algorithm.
    const spawnSequence: { model: Model; inst: number; qty: number }[] = []
    for (const model of models) {
      const qty = items[model.id] ?? 0
      if (qty === 0) continue
      for (let inst = 0; inst < qty; inst++) {
        spawnSequence.push({ model, inst, qty })
      }
    }
    if (spawnOrder === 'random') {
      for (let i = spawnSequence.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1))
        ;[spawnSequence[i], spawnSequence[j]] = [spawnSequence[j], spawnSequence[i]]
      }
    }

    for (const { model, inst } of spawnSequence) {
      const color = modelColor.get(model.id) ?? PALETTE[0]
      const hullJson = hullMode === 'sansRaft'
        ? (model.convexSansRaftHull ?? model.convexHull)
        : model.convexHull
      const localVerts = parseHullLocalPx(hullJson)
      const spawnX = CANVAS_WIDTH_PX * 0.2 + Math.random() * CANVAS_WIDTH_PX * 0.6

      // Create physics body (inflated) + keep original verts for rendering
      let body: Matter.Body
      let visualLocalVerts: Vec2[]
      if (localVerts && localVerts.length >= 3) {
        try {
          body = makePolygonBody(spawnX, spawnY, inflateVerts(localVerts, BODY_MARGIN_PX))
          visualLocalVerts = localVerts;
        } catch {
          ;({ body, visualLocalVerts } = makeRectBody(model, spawnX, spawnY))
        }
      } else {
        ;({ body, visualLocalVerts } = makeRectBody(model, spawnX, spawnY))
      }

      // If there's a saved position for this instance, move the body there
      const saved = savedLayout?.positions.find(
        p => p.modelId === model.id && p.instanceIndex === inst,
      )
      if (saved) {
        Matter.Body.setPosition(body, { x: saved.xMm * PX_PER_MM, y: saved.yMm * PX_PER_MM })
        Matter.Body.setAngle(body, saved.angle)
        Matter.Body.setVelocity(body, { x: 0, y: 0 })
        Matter.Body.setAngularVelocity(body, 0)
      }

      Matter.Composite.add(engine.world, body)
      dynamicBodies.push(body)

      // Spread spawn points above canvas so bodies don't all overlap
      spawnY -= 50 + (localVerts ? Math.max(...localVerts.map(v => Math.abs(v.y))) * 2 : 60)

      // Pixi graphics (drawn in world-space each frame)
      const gfx = new PIXI.Graphics()
      app.stage.addChild(gfx)

      // Label
      const labelStr = model.name.length > 14 ? model.name.slice(0, 12) + '…' : model.name
      const label = new PIXI.Text(labelStr, {
        fontFamily: 'system-ui, -apple-system, sans-serif',
        fontSize: 8,
        fill: 0xffffff,
        align: 'center',
      })
      label.anchor.set(0.5)
      app.stage.addChild(label)

      entries.push({ body, gfx, label, modelId: model.id, instanceIndex: inst, color, visualLocalVerts })
    }

    // ── Simulation state ───────────────────────────────────────────────────
    // Start paused if positions were restored from storage; user clicks to resume.
    pausedRef.current = savedLayout !== null
    notifyPausedRef.current?.(pausedRef.current)
    // Record which items are now in the simulation (for incremental updates)
    prevItemsRef.current = { ...items }
    let settleFrames = 0
    let drag: { body: Matter.Body; ox: number; oy: number } | null = null

    // ── Render helper ──────────────────────────────────────────────────────
    function renderEntries(overlapping: Set<number>) {
      for (const { body, gfx, label, color, visualLocalVerts } of entries) {
        const renderColor = overlapping.has(body.id) ? color : darkenColor(color, 0.45)
        drawBody(gfx, body, visualLocalVerts, renderColor)
        label.position.set(body.position.x, body.position.y)
      }
    }

    // ── Layout persistence ─────────────────────────────────────────────────
    function saveLayout() {
      const layout: SavedLayout = {
        itemsKey: itemsKeyRef.current,
        positions: entries.map(e => ({
          modelId: e.modelId,
          instanceIndex: e.instanceIndex,
          xMm: e.body.position.x / PX_PER_MM,
          yMm: e.body.position.y / PX_PER_MM,
          angle: e.body.angle,
        })),
      }
      localStorage.setItem(LAYOUT_LOCALSTORAGE_KEY, JSON.stringify(layout))
    }
    saveLayoutRef.current = saveLayout

    // ── Drag interaction ───────────────────────────────────────────────────
    app.stage.on('pointerdown', (e: PIXI.FederatedPointerEvent) => {
      // Resume simulation on any click when paused
      if (pausedRef.current) {
        pausedRef.current = false
        notifyPausedRef.current?.(false)
        settleFrames = 0
      }

      const { x, y } = e.global
      const hits = Matter.Query.point(dynamicBodies, { x, y })
      if (hits.length > 0) {
        const hit = hits[0]
        drag = { body: hit, ox: x - hit.position.x, oy: y - hit.position.y }
        Matter.Body.setStatic(hit, true)
        if (pauseOnDragRef.current) {
          pausedRef.current = true
          notifyPausedRef.current?.(true)
        }
      }
    })

    app.stage.on('pointermove', (e: PIXI.FederatedPointerEvent) => {
      if (!drag) return
      const { x, y } = e.global
      Matter.Body.setPosition(drag.body, { x: x - drag.ox, y: y - drag.oy })
    })

    const endDrag = () => {
      if (!drag) return
      Matter.Body.setStatic(drag.body, false)
      Matter.Body.setVelocity(drag.body, { x: 0, y: 0 })
      Matter.Body.setAngularVelocity(drag.body, 0)
      drag = null
      pausedRef.current = false
      notifyPausedRef.current?.(false)
      settleFrames = 0
    }
    app.stage.on('pointerup', endDrag)
    app.stage.on('pointerupoutside', endDrag)

    const canvas = app.view as HTMLCanvasElement
    const onWheel = (e: WheelEvent) => {
      if (!drag) return
      e.preventDefault()
      Matter.Body.setAngle(drag.body, drag.body.angle + e.deltaY * 0.003)
    }
    canvas.addEventListener('wheel', onWheel, { passive: false })

    // ── Ticker ─────────────────────────────────────────────────────────────
    app.ticker.add(() => {
      if (!pausedRef.current) {
        // Cap delta to avoid physics explosion after tab becomes active again
        const deltaMs = Math.min(app.ticker.deltaMS, 50)
        Matter.Engine.update(engine, deltaMs)
      }

      renderEntries(computeOverlapping(entries))

      // Settling detection (skip when dragging or already paused)
      if (!pausedRef.current && !drag && dynamicBodies.length > 0) {
        const allSlow = dynamicBodies.every(
          b => b.speed < SPEED_THRESH && Math.abs(b.angularSpeed) < ANGULAR_THRESH,
        )
        if (allSlow) {
          settleFrames++
          if (settleFrames >= SETTLE_FRAMES) {
            pausedRef.current = true
            notifyPausedRef.current?.(true)
            saveLayout()
          }
        } else {
          settleFrames = 0
        }
      }
    })

    // ── Cleanup ────────────────────────────────────────────────────────────
    return () => {
      canvas.removeEventListener('wheel', onWheel)
      appRef.current = null
      engineRef.current = null
      entriesRef.current = []
      dynamicBodiesRef.current = []
      modelColorRef.current = new Map()
      saveLayoutRef.current = null
      app.destroy(true, { children: true, texture: true, baseTexture: true })
      Matter.Engine.clear(engine)
    }
  }, [spawnOrder, hullMode, resetCount])

  // ── Incremental effect: add/remove bodies when item counts change ──────────
  useEffect(() => {
    const app = appRef.current
    const engine = engineRef.current
    if (!app || !engine) return // simulation not yet initialised

    const prevItems = prevItemsRef.current
    const currItems = items
    const currentModels = modelsRef.current

    const allModelIds = new Set([...Object.keys(prevItems), ...Object.keys(currItems)])

    for (const modelId of allModelIds) {
      const prevQty = prevItems[modelId] ?? 0
      const currQty = currItems[modelId] ?? 0
      if (prevQty === currQty) continue

      const model = currentModels.find(m => m.id === modelId)
      if (!model) continue

      if (currQty > prevQty) {
        // ── Add new instances (from prevQty up to currQty-1) ───────────────
        // Assign a palette colour if this model hasn't appeared before
        if (!modelColorRef.current.has(modelId)) {
          modelColorRef.current.set(modelId, PALETTE[modelColorRef.current.size % PALETTE.length])
        }
        let spawnY = -80
        for (let inst = prevQty; inst < currQty; inst++) {
          const color = modelColorRef.current.get(modelId)!
          const hullJson = hullModeRef.current === 'sansRaft'
            ? (model.convexSansRaftHull ?? model.convexHull)
            : model.convexHull
          const localVerts = parseHullLocalPx(hullJson)
          const spawnX = CANVAS_WIDTH_PX * 0.2 + Math.random() * CANVAS_WIDTH_PX * 0.6

          let body: Matter.Body
          let visualLocalVerts: Vec2[]
          if (localVerts && localVerts.length >= 3) {
            try {
              body = makePolygonBody(spawnX, spawnY, inflateVerts(localVerts, BODY_MARGIN_PX))
              visualLocalVerts = localVerts
            } catch {
              ;({ body, visualLocalVerts } = makeRectBody(model, spawnX, spawnY))
            }
          } else {
            ;({ body, visualLocalVerts } = makeRectBody(model, spawnX, spawnY))
          }

          spawnY -= 50 + (localVerts ? Math.max(...localVerts.map(v => Math.abs(v.y))) * 2 : 60)

          Matter.Composite.add(engine.world, body)
          dynamicBodiesRef.current.push(body)

          const gfx = new PIXI.Graphics()
          app.stage.addChild(gfx)

          const label = new PIXI.Text(model.name.length > 14 ? model.name.slice(0, 12) + '…' : model.name, {
            fontFamily: 'system-ui, -apple-system, sans-serif',
            fontSize: 8,
            fill: 0xffffff,
            align: 'center',
          })
          label.anchor.set(0.5)
          app.stage.addChild(label)

          entriesRef.current.push({ body, gfx, label, modelId, instanceIndex: inst, color, visualLocalVerts })
        }

        pausedRef.current = false // let new bodies fall in

      } else {
        // Remove the models at the top of the build plate first
        for (let i = prevQty; i > currQty; i--) {
          let minY = Infinity
          let toRemove: Entry | null = null
          for (const e of entriesRef.current) {
            if (e.modelId === modelId && e.body.position.y < minY) {
              minY = e.body.position.y
              toRemove = e
            }
          }
          if (!toRemove) break

          Matter.Composite.remove(engine.world, toRemove.body)
          const dynIdx = dynamicBodiesRef.current.indexOf(toRemove.body)
          if (dynIdx >= 0) dynamicBodiesRef.current.splice(dynIdx, 1)

          app.stage.removeChild(toRemove.gfx)
          toRemove.gfx.destroy()
          app.stage.removeChild(toRemove.label)
          toRemove.label.destroy()

          const eIdx = entriesRef.current.indexOf(toRemove)
          if (eIdx >= 0) entriesRef.current.splice(eIdx, 1)
        }

      }
    }

    prevItemsRef.current = { ...currItems }
  }, [itemsKey]) // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <Stack direction="column" spacing={1}>
      <Stack direction="row" spacing={1} alignContent={"center"} alignItems="center">
        <Typography variant="body2" color="text.secondary">
          Printer: Uniformation GK2<br/>
          Print area: {CANVAS_WIDTH_MM} × {CANVAS_HEIGHT_MM} mm
        </Typography>
        <ToggleButtonGroup
          value={spawnOrder}
          exclusive
          onChange={(_e, v: SpawnOrder | null) => {
            if (v) {
              localStorage.removeItem(LAYOUT_LOCALSTORAGE_KEY)
              localStorage.setItem(SPAWN_ORDER_LOCALSTORAGE_KEY, v)
              setSpawnOrder(v)
            }
          }}
          size="small"
        >
          <ToggleButton value="grouped">Grouped</ToggleButton>
          <ToggleButton value="random">Random</ToggleButton>
        </ToggleButtonGroup>
        <Select
          value={hullMode}
          onChange={e => {
            const next = e.target.value as HullMode
            localStorage.removeItem(LAYOUT_LOCALSTORAGE_KEY)
            localStorage.setItem(HULL_MODE_LOCALSTORAGE_KEY, next)
            setHullMode(next)
          }}
          size="small"
        >
          <MenuItem value="convex">Convex hull</MenuItem>
          <MenuItem value="sansRaft">Sans raft hull</MenuItem>
        </Select>
        <Button
          size="large"
          variant="outlined"
          onClick={() => {
            pausedRef.current = true
            notifyPausedRef.current?.(true)
            saveLayoutRef.current?.()
          }}
          disabled={isPaused}
        >
          Save
        </Button>
        <Button
          size="large"
          variant="outlined"
          onClick={() => {
            localStorage.removeItem(LAYOUT_LOCALSTORAGE_KEY)
            setResetCount(c => c + 1)
          }}
        >
          Reset
        </Button>
        <FormGroup>
          <FormControlLabel
            label="Pause on drag"
            control={
              <Checkbox
                checked={pauseOnDrag}
                onChange={() => {
                  const next = !pauseOnDrag
                  localStorage.setItem(PAUSE_ON_DRAG_LOCALSTORAGE_KEY, String(next))
                  setPauseOnDrag(next)
                }}
              />
            }
          />
        </FormGroup>
      </Stack>
      <div
        ref={containerRef}
        style={{
          width: CANVAS_WIDTH_PX,
          height: CANVAS_HEIGHT_PX,
          borderRadius: 12,
          overflow: 'hidden',
          boxShadow: '0 4px 24px rgba(0,0,0,0.4)',
          cursor: 'default',
        }}
      />
    </Stack>
  )
}
