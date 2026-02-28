import React, { Suspense, useEffect } from 'react'
import { Canvas, useLoader } from '@react-three/fiber'
import { OrbitControls, Bounds, Center, Html } from '@react-three/drei'
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

function StlModel({ url, color }: { url: string; color: string }) {
  const geometry = useLoader(STLLoader, url)
  return (
    <Bounds fit clip observe>
      <Center>
        <mesh geometry={geometry}>
          <meshStandardMaterial color={color} roughness={0.55} metalness={0.15} />
        </mesh>
      </Center>
    </Bounds>
  )
}

function ObjModel({ url, color }: { url: string; color: string }) {
  const obj = useLoader(OBJLoader, url)
  useEffect(() => {
    const mat = new THREE.MeshStandardMaterial({ color, roughness: 0.55, metalness: 0.15 })
    obj.traverse((child) => {
      if ((child as THREE.Mesh).isMesh) (child as THREE.Mesh).material = mat
    })
    return () => mat.dispose()
  }, [obj, color])
  return (
    <Bounds fit clip observe>
      <Center>
        <primitive object={obj} />
      </Center>
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
}

export default function ModelViewer({ fileUrl, fileType }: ModelViewerProps) {
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
        camera={{ position: [0, 0, 5], fov: 45 }}
        gl={{ antialias: true }}
        style={containerStyle}
      >
        <Lighting />
        <Suspense
          fallback={
            <Html center>
              <span style={{ fontSize: '0.85rem', color: '#64748b', whiteSpace: 'nowrap' }}>
                Loading model…
              </span>
            </Html>
          }
        >
          {type === 'stl' && <StlModel url={fileUrl} color={color} />}
          {type === 'obj' && <ObjModel url={fileUrl} color={color} />}
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
