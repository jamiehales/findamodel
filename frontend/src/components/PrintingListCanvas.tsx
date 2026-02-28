import { useEffect, useMemo, useRef } from 'react'
import * as PIXI from 'pixi.js'
import Matter from 'matter-js'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import type { Model } from '../lib/api'
import type { PrintingList } from '../lib/printingList'

// ── Constants ─────────────────────────────────────────────────────────────────

const CANVAS_MM = 150
const PX_PER_MM = 4
const CANVAS_PX = CANVAS_MM * PX_PER_MM // 600 px

const WALL_THICKNESS = 200
const SPEED_THRESH = 0.12
const ANGULAR_THRESH = 0.008
const SETTLE_FRAMES = 90 // ~1.5 s at 60 fps
const MAX_BODY_HALF_PX = (CANVAS_PX * 0.65) / 2 // cap body half-extent
const LAYOUT_KEY = 'findamodel.printingListLayout'

// Physics body inflation — when two bodies touch they have BODY_GAP_MM visual gap.
const BODY_GAP_MM = 4
const BODY_MARGIN_PX = (BODY_GAP_MM / 2) * PX_PER_MM // 2 mm per body side

// Overlap-detection hull inflation — bodies within BODY_OVERLAP_MM of each other
// are considered overlapping and rendered darker.
const BODY_OVERLAP_MM = 1
const BODY_OVERLAP_PX = (BODY_OVERLAP_MM / 2) * PX_PER_MM // 1 mm per body side

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
    x: number
    y: number
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
  /** Hull inflated by BODY_OVERLAP_PX per side, for overlap detection. */
  overlapLocalVerts: Vec2[]
}

interface Props {
  models: Model[]
  items: PrintingList
}

// ── Hull helpers ──────────────────────────────────────────────────────────────

/**
 * Parse a JSON hull string ("[[x,z],[x,z],...]") into local-space pixel
 * vertices centred at the origin and clamped to MAX_BODY_HALF_PX.
 */
function parseHullLocalPx(hullJson: string | null): Vec2[] | null {
  if (!hullJson) return null
  try {
    const raw: [number, number][] = JSON.parse(hullJson)
    if (raw.length < 3) return null

    // Hull coords are [x, z] in model space (assumed mm for 3-D printing STLs).
    // Map z → canvas y.
    const pts = raw.map(([x, z]): Vec2 => ({ x: x * PX_PER_MM, y: z * PX_PER_MM }))

    // Centre at vertex centroid (close enough for convex polygons).
    const cx = pts.reduce((s, p) => s + p.x, 0) / pts.length
    const cy = pts.reduce((s, p) => s + p.y, 0) / pts.length
    const centred = pts.map(p => ({ x: p.x - cx, y: p.y - cy }))

    // Scale down if any vertex exceeds the allowed half-extent.
    const maxHalf = Math.max(...centred.flatMap(p => [Math.abs(p.x), Math.abs(p.y)]), 1)
    const scale = maxHalf > MAX_BODY_HALF_PX ? MAX_BODY_HALF_PX / maxHalf : 1

    return scale === 1 ? centred : centred.map(p => ({ x: p.x * scale, y: p.y * scale }))
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

const BODY_OPTIONS: Matter.IChamferableBodyDefinition = {
  restitution: 0.05,
  friction: 0.5,
  frictionAir: 0.025,
  density: 0.001,
}

function makePolygonBody(cx: number, cy: number, localVerts: Vec2[]): Matter.Body {
  // fromVertices centres the body at (cx, cy) using the centroid of the passed
  // vertices. Since localVerts are already centred at origin, their centroid ≈ 0,
  // and Matter.js will correctly place the body at (cx, cy).
  return Matter.Bodies.fromVertices(cx, cy, localVerts as Matter.Vector[], BODY_OPTIONS)
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
): { body: Matter.Body; visualLocalVerts: Vec2[]; overlapLocalVerts: Vec2[] } {
  const w = Math.min(Math.max((model.dimensionXMm ?? 20) * PX_PER_MM, 16), MAX_BODY_HALF_PX * 2)
  const h = Math.min(Math.max((model.dimensionZMm ?? 20) * PX_PER_MM, 16), MAX_BODY_HALF_PX * 2)
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
  const ohw = hw + BODY_OVERLAP_PX
  const ohh = hh + BODY_OVERLAP_PX
  const overlapLocalVerts: Vec2[] = [
    { x: -ohw, y: -ohh },
    { x: ohw, y: -ohh },
    { x: ohw, y: ohh },
    { x: -ohw, y: ohh },
  ]
  return { body, visualLocalVerts, overlapLocalVerts }
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
  const worldVerts = entries.map(e => toWorldVerts(e.body, e.overlapLocalVerts))
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
}

// ── Component ─────────────────────────────────────────────────────────────────

export default function PrintingListCanvas({ models, items }: Props) {
  const containerRef = useRef<HTMLDivElement>(null)
  const itemsKey = useMemo(() => JSON.stringify(items), [items])

  useEffect(() => {
    const container = containerRef.current
    if (!container) return

    // ── Pixi application ───────────────────────────────────────────────────
    const app = new PIXI.Application({
      width: CANVAS_PX,
      height: CANVAS_PX,
      backgroundColor: 0x0f172a,
      antialias: true,
      resolution: Math.min(window.devicePixelRatio || 1, 2),
      autoDensity: true,
    })
    container.appendChild(app.view as HTMLCanvasElement)
    app.stage.eventMode = 'static'
    app.stage.hitArea = new PIXI.Rectangle(0, 0, CANVAS_PX, CANVAS_PX)

    // ── Matter.js engine ───────────────────────────────────────────────────
    const engine = Matter.Engine.create({ gravity: { x: 0, y: 1 } })

    const ground = Matter.Bodies.rectangle(
      CANVAS_PX / 2,
      CANVAS_PX + WALL_THICKNESS / 2,
      CANVAS_PX + WALL_THICKNESS * 2,
      WALL_THICKNESS,
      { isStatic: true },
    )
    const wallLeft = Matter.Bodies.rectangle(
      -WALL_THICKNESS / 2,
      CANVAS_PX / 2,
      WALL_THICKNESS,
      CANVAS_PX + WALL_THICKNESS * 2,
      { isStatic: true },
    )
    const wallRight = Matter.Bodies.rectangle(
      CANVAS_PX + WALL_THICKNESS / 2,
      CANVAS_PX / 2,
      WALL_THICKNESS,
      CANVAS_PX + WALL_THICKNESS * 2,
      { isStatic: true },
    )
    Matter.Composite.add(engine.world, [ground, wallLeft, wallRight])

    // ── Background ─────────────────────────────────────────────────────────
    const GRID_PX = 10 * PX_PER_MM
    const gridGfx = new PIXI.Graphics()
    gridGfx.lineStyle(0.5, 0x334155, 0.6)
    for (let x = GRID_PX; x < CANVAS_PX; x += GRID_PX) gridGfx.moveTo(x, 0).lineTo(x, CANVAS_PX)
    for (let y = GRID_PX; y < CANVAS_PX; y += GRID_PX) gridGfx.moveTo(0, y).lineTo(CANVAS_PX, y)
    app.stage.addChild(gridGfx)

    const borderGfx = new PIXI.Graphics()
    borderGfx.lineStyle(2, 0x64748b, 1)
    borderGfx.drawRect(0, 0, CANVAS_PX, CANVAS_PX)
    app.stage.addChild(borderGfx)

    // ── Saved layout ───────────────────────────────────────────────────────
    let savedLayout: SavedLayout | null = null
    try {
      const raw = localStorage.getItem(LAYOUT_KEY)
      if (raw) {
        const parsed: SavedLayout = JSON.parse(raw)
        if (parsed.itemsKey === itemsKey) savedLayout = parsed
      }
    } catch { /* ignore */ }

    // ── Build bodies + graphics ────────────────────────────────────────────
    const entries: Entry[] = []
    const dynamicBodies: Matter.Body[] = []

    const modelColor = new Map<string, number>()
    models.forEach((m, i) => modelColor.set(m.id, PALETTE[i % PALETTE.length]))

    let spawnY = -40 // bodies spawn above the canvas and fall in

    for (const model of models) {
      const qty = items[model.id] ?? 0
      if (qty === 0) continue

      const color = modelColor.get(model.id) ?? PALETTE[0]
      const localVerts = parseHullLocalPx(model.convexHull)

      for (let inst = 0; inst < qty; inst++) {
        const spawnX = CANVAS_PX * 0.2 + Math.random() * CANVAS_PX * 0.6

        // Create physics body (inflated) + keep original verts for rendering
        let body: Matter.Body
        let visualLocalVerts: Vec2[]
        let overlapLocalVerts: Vec2[]
        if (localVerts && localVerts.length >= 3) {
          try {
            body = makePolygonBody(spawnX, spawnY, inflateVerts(localVerts, BODY_MARGIN_PX))
            visualLocalVerts = localVerts
            overlapLocalVerts = inflateVerts(localVerts, BODY_OVERLAP_PX)
          } catch {
            ;({ body, visualLocalVerts, overlapLocalVerts } = makeRectBody(model, spawnX, spawnY))
          }
        } else {
          ;({ body, visualLocalVerts, overlapLocalVerts } = makeRectBody(model, spawnX, spawnY))
        }

        // If there's a saved position for this instance, move the body there
        const saved = savedLayout?.positions.find(
          p => p.modelId === model.id && p.instanceIndex === inst,
        )
        if (saved) {
          Matter.Body.setPosition(body, { x: saved.x, y: saved.y })
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
        const nameShort = model.name.length > 14 ? model.name.slice(0, 12) + '…' : model.name
        const labelStr = qty > 1 ? `${nameShort} #${inst + 1}` : nameShort
        const label = new PIXI.Text(labelStr, {
          fontFamily: 'system-ui, -apple-system, sans-serif',
          fontSize: 8,
          fill: 0xffffff,
          align: 'center',
        })
        label.anchor.set(0.5)
        app.stage.addChild(label)

        entries.push({ body, gfx, label, modelId: model.id, instanceIndex: inst, color, visualLocalVerts, overlapLocalVerts })
      }
    }

    // ── Simulation state ───────────────────────────────────────────────────
    // Start paused if positions were restored from storage; user clicks to resume.
    let paused = savedLayout !== null
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
        itemsKey,
        positions: entries.map(e => ({
          modelId: e.modelId,
          instanceIndex: e.instanceIndex,
          x: e.body.position.x,
          y: e.body.position.y,
          angle: e.body.angle,
        })),
      }
      localStorage.setItem(LAYOUT_KEY, JSON.stringify(layout))
    }

    // ── Drag interaction ───────────────────────────────────────────────────
    app.stage.on('pointerdown', (e: PIXI.FederatedPointerEvent) => {
      // Resume simulation on any click when paused
      if (paused) {
        paused = false
        settleFrames = 0
      }

      const { x, y } = e.global
      const hits = Matter.Query.point(dynamicBodies, { x, y })
      if (hits.length > 0) {
        const hit = hits[0]
        drag = { body: hit, ox: x - hit.position.x, oy: y - hit.position.y }
        Matter.Body.setStatic(hit, true)
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
      settleFrames = 0
    }
    app.stage.on('pointerup', endDrag)
    app.stage.on('pointerupoutside', endDrag)

    // ── Ticker ─────────────────────────────────────────────────────────────
    app.ticker.add(() => {
      if (!paused) {
        // Cap delta to avoid physics explosion after tab becomes active again
        const deltaMs = Math.min(app.ticker.deltaMS, 50)
        Matter.Engine.update(engine, deltaMs)
      }

      renderEntries(computeOverlapping(entries))

      // Settling detection (skip when dragging or already paused)
      if (!paused && !drag && dynamicBodies.length > 0) {
        const allSlow = dynamicBodies.every(
          b => b.speed < SPEED_THRESH && Math.abs(b.angularSpeed) < ANGULAR_THRESH,
        )
        if (allSlow) {
          settleFrames++
          if (settleFrames >= SETTLE_FRAMES) {
            paused = true
            saveLayout()
          }
        } else {
          settleFrames = 0
        }
      }
    })

    // ── Cleanup ────────────────────────────────────────────────────────────
    return () => {
      app.destroy(true, { children: true, texture: true, baseTexture: true })
      Matter.Engine.clear(engine)
    }
  }, [models, itemsKey])

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: '0.5rem', alignItems: 'flex-start' }}>
      <Typography sx={{ color: '#94a3b8', fontSize: '0.8rem', fontWeight: 500 }}>
        Print area: {CANVAS_MM} × {CANVAS_MM} mm &nbsp;·&nbsp; Click to restart simulation &nbsp;·&nbsp; Drag to reposition
      </Typography>
      <div
        ref={containerRef}
        style={{
          width: CANVAS_PX,
          maxWidth: '100%',
          aspectRatio: '1',
          borderRadius: 12,
          overflow: 'hidden',
          boxShadow: '0 4px 24px rgba(0,0,0,0.4)',
          cursor: 'default',
        }}
      />
    </Box>
  )
}
