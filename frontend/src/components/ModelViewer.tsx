import React, { useMemo } from 'react'
import { Canvas } from '@react-three/fiber'
import { OrbitControls, Bounds, Html } from '@react-three/drei'
import * as THREE from 'three'
import { useGeometry } from '../lib/queries'

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

interface ConvexHullLineProps {
  coordinates: Array<[number, number]> | null
}

function ConvexHullLine({ coordinates }: ConvexHullLineProps) {
  if (!coordinates || coordinates.length < 2) return null

  const points = coordinates.map(([x, z]) => new THREE.Vector3(x, 0, z))
  // Close the polygon if not already closed
  if (points[0].distanceTo(points[points.length - 1]) > 0.001) {
    points.push(points[0].clone())
  }

  const geometry = new THREE.BufferGeometry().setFromPoints(points)

  return (
    <lineSegments geometry={geometry} position={[0, 0.01, 0]}>
      <lineBasicMaterial color="#818cf8" />
    </lineSegments>
  )
}

interface ConvexHullPolygonProps {
  coordinates: Array<[number, number]> | null
}

function ConvexHullPolygon({ coordinates }: ConvexHullPolygonProps) {
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
    <mesh geometry={geometry} position={[0, 0.005, 0]}>
      <meshBasicMaterial color="#818cf8" transparent opacity={0.15} side={THREE.DoubleSide} />
    </mesh>
  )
}

interface GeometryModelProps {
  modelId: string
  color: string
  convexHull: string | null
}

function GeometryModel({ modelId, color, convexHull }: GeometryModelProps) {
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

  return (
    <Bounds fit clip observe>
      <group>
        <mesh geometry={bufferGeometry}>
          <meshStandardMaterial color={color} roughness={0.55} metalness={0.15} />
        </mesh>
        <ConvexHullPolygon coordinates={hullCoords} />
        <ConvexHullLine coordinates={hullCoords} />
      </group>
    </Bounds>
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
}

export default function ModelViewer({ modelId, fileType, convexHull }: ModelViewerProps) {
  const color = ACCENT[fileType.toLowerCase()] ?? '#94a3b8'

  const errorFallback = (
    <div style={containerStyle}>
      <span style={{ fontSize: '2rem', opacity: 0.3 }}>⬡</span>
      <span style={{ fontSize: '0.85rem', color: '#64748b' }}>Could not load 3D model</span>
    </div>
  )

  return (
    <ViewerErrorBoundary fallback={errorFallback}>
      <Canvas
        camera={{ position: [0, 5, -5], fov: 45 }}
        gl={{ antialias: true }}
        style={containerStyle}
      >
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
          <GeometryModel modelId={modelId} color={color} convexHull={convexHull ?? null} />
        </React.Suspense>
        <OrbitControls
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
