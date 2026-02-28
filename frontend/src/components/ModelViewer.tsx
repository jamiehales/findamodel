import React, { Suspense, useEffect } from 'react'
import { Canvas, useLoader, useFrame } from '@react-three/fiber'
import { OrbitControls, Bounds, Html } from '@react-three/drei'
import { STLLoader } from 'three/addons/loaders/STLLoader.js'
import { OBJLoader } from 'three/addons/loaders/OBJLoader.js'
import * as THREE from 'three'

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
    <gridHelper args={[20, 20, '#3a4559', '#1e293b']} position={[0, 0, 0]} rotation={[0, 0, 0]} />
  )
}

function CameraController() {
  useFrame(({ camera }) => {
    camera.lookAt(0, 0, 0)
  })
  return null
}

interface ConvexHullLineProps {
  coordinates: Array<[number, number]> | null
}

function ConvexHullLine({ coordinates }: ConvexHullLineProps) {
  if (!coordinates || coordinates.length < 2) return null

  // Calculate center of hull points to apply same offset as model
  let transformedCoords = coordinates.map(([x, z]) => [x, z] as [number, number])
  if (transformedCoords.length > 0) {
    const centerX = transformedCoords.reduce((sum, [x]) => sum + x, 0) / transformedCoords.length
    const centerZ = transformedCoords.reduce((sum, [, z]) => sum + z, 0) / transformedCoords.length
    // Offset coordinates to center at origin
    transformedCoords = transformedCoords.map(([x, z]) => [x - centerX, z - centerZ] as [number, number])
  }
  
  const points = transformedCoords.map(([x, z]) => new THREE.Vector3(x, 0, z))
  // Close the polygon
  if (points.length > 0 && (points[0].x !== points[points.length - 1].x || points[0].z !== points[points.length - 1].z)) {
    points.push(points[0].clone())
  }

  const geometry = new THREE.BufferGeometry().setFromPoints(points)
  
  return (
    <lineSegments geometry={geometry} position={[0, 0.01, 0]}>
      <lineBasicMaterial color="#818cf8" linewidth={2} />
    </lineSegments>
  )
}

interface ConvexHullPolygonProps {
  coordinates: Array<[number, number]> | null
}

function ConvexHullPolygon({ coordinates }: ConvexHullPolygonProps) {
  if (!coordinates || coordinates.length < 3) return null

  // Calculate center of hull points to apply same offset as model
  let transformedCoords = coordinates.map(([x, z]) => [x, z] as [number, number])
  if (transformedCoords.length > 0) {
    const centerX = transformedCoords.reduce((sum, [x]) => sum + x, 0) / transformedCoords.length
    const centerZ = transformedCoords.reduce((sum, [, z]) => sum + z, 0) / transformedCoords.length
    // Offset coordinates to center at origin
    transformedCoords = transformedCoords.map(([x, z]) => [x - centerX, z - centerZ] as [number, number])
  }
  
  const shape = new THREE.Shape()
  shape.moveTo(transformedCoords[0][0], transformedCoords[0][1])
  for (let i = 1; i < transformedCoords.length; i++) {
    shape.lineTo(transformedCoords[i][0], transformedCoords[i][1])
  }
  shape.lineTo(transformedCoords[0][0], transformedCoords[0][1])

  const geometry = new THREE.ShapeGeometry(shape)
  // Rotate geometry from XY plane to XZ plane (rotateX by 90 degrees)
  geometry.rotateX(Math.PI / 2)
  
  return (
    <mesh geometry={geometry} position={[0, 0.005, 0]}>
      <meshBasicMaterial color="#818cf8" transparent opacity={0.15} side={THREE.DoubleSide} />
    </mesh>
  )
}

function StlModel({ url, color, convexHull }: { url: string; color: string; convexHull: string | null }) {
  const geometry = useLoader(STLLoader, url)
  let convexHullCoordinates: Array<[number, number]> | null = null
  
  try {
    if (convexHull) {
      convexHullCoordinates = JSON.parse(convexHull)
    }
  } catch {
    convexHullCoordinates = null
  }

  // Convert from Z-up to Y-up
  useEffect(() => {
    geometry.rotateX(Math.PI / 2)
    geometry.rotateZ(Math.PI)
    geometry.computeBoundingBox()
    geometry.computeBoundingSphere()
    
    // Center the geometry at the origin in X and Z, keep Y at 0
    const sphere = geometry.boundingSphere
    if (sphere) {
      geometry.translate(-sphere.center.x, 0, -sphere.center.z)
      console.log('Translated STL to origin')
    }
  }, [geometry])

  return (
    <Bounds fit clip observe>
      <group position={[0, 0, 0]}>
        <mesh geometry={geometry}>
          <meshStandardMaterial color={color} roughness={0.55} metalness={0.15} />
        </mesh>
        <ConvexHullPolygon coordinates={convexHullCoordinates} />
        <ConvexHullLine coordinates={convexHullCoordinates} />
      </group>
    </Bounds>
  )
}

function ObjModel({ url, color, convexHull }: { url: string; color: string; convexHull: string | null }) {
  const obj = useLoader(OBJLoader, url)
  let convexHullCoordinates: Array<[number, number]> | null = null
  
  try {
    if (convexHull) {
      convexHullCoordinates = JSON.parse(convexHull)
    }
  } catch {
    convexHullCoordinates = null
  }

  useEffect(() => {
    const mat = new THREE.MeshStandardMaterial({ color, roughness: 0.55, metalness: 0.15 })
    obj.traverse((child) => {
      if ((child as THREE.Mesh).isMesh) {
        (child as THREE.Mesh).material = mat
        // Convert from Z-up to Y-up by rotating around X axis
        ;(child as THREE.Mesh).geometry.rotateX(Math.PI / 2)
        ;(child as THREE.Mesh).geometry.rotateZ(Math.PI)
        ;(child as THREE.Mesh).geometry.computeBoundingBox()
        ;(child as THREE.Mesh).geometry.computeBoundingSphere()
        
        // Center the geometry at the origin in X and Z, keep Y at 0
        const mesh = child as THREE.Mesh
        const sphere = mesh.geometry.boundingSphere
        if (sphere) {
          mesh.geometry.translate(-sphere.center.x, 0, -sphere.center.z)
          console.log('Translated OBJ mesh to origin')
        }
      }
    })
    return () => mat.dispose()
  }, [obj, color])

  return (
    <Bounds fit clip observe>
      <group position={[0, 0, 0]}>
        <primitive object={obj} />
        <ConvexHullPolygon coordinates={convexHullCoordinates} />
        <ConvexHullLine coordinates={convexHullCoordinates} />
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
  fileUrl: string
  fileType: string
  convexHull?: string | null
}

export default function ModelViewer({ fileUrl, fileType, convexHull }: ModelViewerProps) {
  const type = fileType.toLowerCase()
  const color = ACCENT[type] ?? '#94a3b8'

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
        <CameraController />
        <Suspense
          fallback={
            <Html center>
              <span style={{ fontSize: '0.85rem', color: '#64748b', whiteSpace: 'nowrap' }}>
                Loading model…
              </span>
            </Html>
          }
        >
          {type === 'stl' && <StlModel url={fileUrl} color={color} convexHull={convexHull ?? null} />}
          {type === 'obj' && <ObjModel url={fileUrl} color={color} convexHull={convexHull ?? null} />}
        </Suspense>
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
