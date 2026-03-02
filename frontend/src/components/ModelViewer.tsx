import React, { useEffect, useMemo } from 'react'
import { Canvas, useThree } from '@react-three/fiber'
import { OrbitControls, Html } from '@react-three/drei'
import * as THREE from 'three'
import { useGeometry, useModel } from '../lib/queries'

const ACCENT: Record<string, string> = {
  stl: '#818cf8',
  obj: '#34d399',
}

function Lighting() {
  return (
    <>
      <ambientLight intensity={0.4} />
      <directionalLight position={[5, 8, 5]} intensity={1.2} />
      <directionalLight position={[-4, 2, -2]} intensity={0.4} />
      <directionalLight position={[0, -3, -5]} intensity={0.25} />
    </>
  )
}

function Grid() {
  return (
    <gridHelper args={[25.4 * 2, 2, '#3a4559', '#1e293b']} position={[0, 0, 0]} />
  )
}

interface HullLineProps {
  coordinates: Array<[number, number]> | null
  color: string
  yOffset?: number
}

function HullLine({ coordinates, color, yOffset = 0 }: HullLineProps) {
  if (!coordinates || coordinates.length < 2) return null

  const points = coordinates.map(([x, z]) => new THREE.Vector3(x, yOffset, z))
  // Close the polygon if not already closed
  if (points[0].distanceTo(points[points.length - 1]) > 0.001) {
    points.push(points[0].clone())
  }

  const geometry = new THREE.BufferGeometry().setFromPoints(points)

  return (
    <lineSegments geometry={geometry} position={[0, 0.01, 0]}>
      <lineBasicMaterial color={color} />
    </lineSegments>
  )
}

interface HullPolygonProps {
  coordinates: Array<[number, number]> | null
  color: string
  yOffset?: number
  opacity?: number
}

function HullPolygon({ coordinates, color, yOffset = 0, opacity = 0.15 }: HullPolygonProps) {
  if (!coordinates || coordinates.length < 3) return null

  const shape = new THREE.Shape()
  shape.moveTo(coordinates[0][0], coordinates[0][1])
  for (let i = 1; i < coordinates.length; i++) {
    shape.lineTo(coordinates[i][0], coordinates[i][1])
  }
  shape.lineTo(coordinates[0][0], coordinates[0][1])

  const geometry = new THREE.ShapeGeometry(shape)
  geometry.rotateX(Math.PI / 2)

  return (
    <mesh geometry={geometry} position={[0, yOffset + 0.005, 0]}>
      <meshBasicMaterial color={color} transparent opacity={opacity} side={THREE.DoubleSide} />
    </mesh>
  )
}

function CameraInit({ position, target }: { position: THREE.Vector3Tuple; target: THREE.Vector3Tuple }) {
  const camera = useThree(state => state.camera)
  useEffect(() => {
    camera.position.set(...position)
    camera.lookAt(...target)
  }, []) // eslint-disable-line react-hooks/exhaustive-deps
  return null
}

interface GeometryModelProps {
  modelId: string
  color: string
  convexHull: string | null
  convexSansRaftHull: string | null
  raftOffsetMm: number
}

function GeometryModel({ modelId, color, convexHull, convexSansRaftHull, raftOffsetMm }: GeometryModelProps) {
  const { data } = useGeometry(modelId)

  const bufferGeometry = useMemo(() => {
    const geo = new THREE.BufferGeometry()
    geo.setAttribute('position', new THREE.Float32BufferAttribute(data.positions, 3))
    geo.setAttribute('normal', new THREE.Float32BufferAttribute(data.normals, 3))
    return geo
  }, [data])

  const hullCoords = useMemo((): Array<[number, number]> | null => {
    if (!convexHull) return null
    try { return JSON.parse(convexHull) }
    catch { return null }
  }, [convexHull])

  const sansRaftCoords = useMemo((): Array<[number, number]> | null => {
    if (!convexSansRaftHull) return null
    try { return JSON.parse(convexSansRaftHull) }
    catch { return null }
  }, [convexSansRaftHull])

  return (
    <group>
      <mesh geometry={bufferGeometry}>
        <meshStandardMaterial color={color} roughness={0.55} metalness={0.15} />
      </mesh>
      <HullPolygon coordinates={hullCoords} color="#818cf8" />
      <HullLine coordinates={hullCoords} color="#818cf8" />
      <HullPolygon coordinates={sansRaftCoords} color="#f59e0b" yOffset={raftOffsetMm} opacity={0.18} />
      <HullLine coordinates={sansRaftCoords} color="#f59e0b" yOffset={raftOffsetMm} />
    </group>
  )
}

interface ErrorBoundaryState { hasError: boolean }

class ViewerErrorBoundary extends React.Component<
  React.PropsWithChildren<{ fallback: React.ReactNode }>,
  ErrorBoundaryState
> {
  constructor(props: React.PropsWithChildren<{ fallback: React.ReactNode }>) {
    super(props)
    this.state = { hasError: false }
  }
  static getDerivedStateFromError(): ErrorBoundaryState {
    return { hasError: true }
  }
  render() {
    if (this.state.hasError) return this.props.fallback
    return this.props.children
  }
}

interface ModelViewerProps {
  modelId: string
  fileType: string   // used for accent colour only
  convexHull?: string | null
  convexSansRaftHull?: string | null
}

export default function ModelViewer({ modelId, fileType, convexHull, convexSansRaftHull }: ModelViewerProps) {
  const { data: model, isPending, isError } = useModel(modelId)
  const color = ACCENT[fileType.toLowerCase()] ?? '#94a3b8'

  const errorFallback = (
    <div style={containerStyle}>
      <span style={{ fontSize: '2rem', opacity: 0.3 }}>⬡</span>
      <span style={{ fontSize: '0.85rem', color: '#64748b' }}>Could not load 3D model</span>
    </div>
  )

  if (isError) return errorFallback

  if (isPending || model.dimensionXMm == null || model.dimensionYMm == null || model.dimensionZMm == null) {
    return (
      <div style={{ ...containerStyle, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        <span style={{ fontSize: '0.85rem', color: '#64748b' }}>
          {isPending ? 'Loading model…' : 'Model dimensions not yet available'}
        </span>
      </div>
    )
  }

  const maxDimension = Math.max(model.dimensionXMm, model.dimensionYMm, model.dimensionZMm)

  const orbitTarget: [number, number, number] = [
    model.sphereCentreX ?? 0,
    (model.sphereCentreY ?? model.dimensionYMm) / 2,
    model.sphereCentreZ ?? 0,
  ]

  const cameraPos: [number, number, number] = [
    orbitTarget[0] + maxDimension,
    orbitTarget[1] + maxDimension,
    orbitTarget[2] - maxDimension,
  ]

  return (
    <ViewerErrorBoundary fallback={errorFallback}>
      <Canvas
        camera={{ fov: 45 }}
        gl={{ antialias: true }}
        style={containerStyle}
      >
        <CameraInit position={cameraPos} target={orbitTarget} />
        <Lighting />
        <Grid />
        <React.Suspense
          fallback={
            <Html center>
              <span style={{ fontSize: '0.85rem', color: '#64748b', whiteSpace: 'nowrap' }}>
                Loading model…
              </span>
            </Html>
          }
        >
          <GeometryModel modelId={modelId} color={color} convexHull={convexHull ?? null} convexSansRaftHull={convexSansRaftHull ?? null} raftOffsetMm={model.raftOffsetMm} />
        </React.Suspense>
        <OrbitControls
          target={orbitTarget}
          enableDamping
          dampingFactor={0.08}
          minDistance={0.5}
          maxDistance={200}
          mouseButtons={{
            LEFT: THREE.MOUSE.ROTATE,
            MIDDLE: THREE.MOUSE.DOLLY,
            RIGHT: THREE.MOUSE.PAN,
          }}
        />
      </Canvas>
    </ViewerErrorBoundary>
  )
}

const containerStyle: React.CSSProperties = {
  width: '100%',
  height: '100%',
  background: '#0f172a',
}
