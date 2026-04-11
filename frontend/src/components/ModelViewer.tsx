import React, { useEffect, useMemo, useState } from 'react';
import { Canvas, useThree } from '@react-three/fiber';
import { OrbitControls, Html } from '@react-three/drei';
import * as THREE from 'three';
import { useBodyGeometry, useGeometry, useModel, useSupportGeometry } from '../lib/queries';

const DEFAULT_VIEW_DIRECTION = new THREE.Vector3(1, 0.8, -1).normalize();
const FRAMING_PADDING = 1.15;

const MODEL_COLOR = '#818cf8';

function Lighting() {
  return (
    <>
      <ambientLight intensity={0.4} />
      <directionalLight position={[5, 8, 5]} intensity={1.2} />
      <directionalLight position={[-4, 2, -2]} intensity={0.4} />
      <directionalLight position={[0, -3, -5]} intensity={0.25} />
    </>
  );
}

function Grid() {
  return <gridHelper args={[25.4 * 2, 2, '#3a4559', '#1e293b']} position={[0, 0, 0]} />;
}

interface HullLineProps {
  coordinates: Array<[number, number]> | null;
  color: string;
  yOffset?: number;
}

function HullLine({ coordinates, color, yOffset = 0 }: HullLineProps) {
  if (!coordinates || coordinates.length < 2) return null;

  const points = coordinates.map(([x, z]) => new THREE.Vector3(x, yOffset, z));
  const geometry = new THREE.BufferGeometry().setFromPoints(points);

  return (
    <lineLoop geometry={geometry} position={[0, 0.01, 0]}>
      <lineBasicMaterial color={color} />
    </lineLoop>
  );
}

interface HullPolygonProps {
  coordinates: Array<[number, number]> | null;
  color: string;
  yOffset?: number;
  opacity?: number;
}

function HullPolygon({ coordinates, color, yOffset = 0, opacity = 0.15 }: HullPolygonProps) {
  if (!coordinates || coordinates.length < 3) return null;

  const shape = new THREE.Shape();
  shape.moveTo(coordinates[0][0], coordinates[0][1]);
  for (let i = 1; i < coordinates.length; i++) {
    shape.lineTo(coordinates[i][0], coordinates[i][1]);
  }
  shape.lineTo(coordinates[0][0], coordinates[0][1]);

  const geometry = new THREE.ShapeGeometry(shape);
  geometry.rotateX(Math.PI / 2);

  return (
    <mesh geometry={geometry} position={[0, yOffset + 0.005, 0]}>
      <meshBasicMaterial color={color} transparent opacity={opacity} side={THREE.DoubleSide} />
    </mesh>
  );
}

function CameraInit({
  target,
  halfExtents,
  direction,
}: {
  target: THREE.Vector3Tuple;
  halfExtents: THREE.Vector3;
  direction: THREE.Vector3;
}) {
  const camera = useThree((state) => state.camera);
  const size = useThree((state) => state.size);

  useEffect(() => {
    const distance =
      camera instanceof THREE.PerspectiveCamera
        ? calculateCameraDistanceForBox(halfExtents, direction, camera.fov, camera.aspect)
        : Math.max(halfExtents.length() * 2, 1);

    camera.position.set(
      target[0] + direction.x * distance,
      target[1] + direction.y * distance,
      target[2] + direction.z * distance,
    );
    camera.lookAt(...target);
  }, [camera, direction, halfExtents, size.height, size.width, target]);

  return null;
}

function calculateCameraDistanceForBox(
  halfExtents: THREE.Vector3,
  direction: THREE.Vector3,
  fovDegrees: number,
  aspect: number,
) {
  const verticalFov = THREE.MathUtils.degToRad(fovDegrees);
  const halfVerticalFov = verticalFov / 2;
  const halfHorizontalFov = Math.atan(Math.tan(halfVerticalFov) * aspect);

  const toCamera = direction.clone().normalize();
  const worldUp =
    Math.abs(toCamera.dot(new THREE.Vector3(0, 1, 0))) > 0.999
      ? new THREE.Vector3(0, 0, 1)
      : new THREE.Vector3(0, 1, 0);
  const right = new THREE.Vector3().crossVectors(worldUp, toCamera).normalize();
  const up = new THREE.Vector3().crossVectors(toCamera, right).normalize();

  const tanHalfHorizontal = Math.tan(halfHorizontalFov);
  const tanHalfVertical = Math.tan(halfVerticalFov);

  let requiredDistance = 0;
  const corner = new THREE.Vector3();
  for (const sx of [-1, 1]) {
    for (const sy of [-1, 1]) {
      for (const sz of [-1, 1]) {
        corner.set(sx * halfExtents.x, sy * halfExtents.y, sz * halfExtents.z);

        const x = Math.abs(corner.dot(right));
        const y = Math.abs(corner.dot(up));
        const z = corner.dot(toCamera);

        const dForX = z + x / tanHalfHorizontal;
        const dForY = z + y / tanHalfVertical;
        requiredDistance = Math.max(requiredDistance, dForX, dForY);
      }
    }
  }

  return Math.max(requiredDistance, 0.001) * FRAMING_PADDING;
}

interface GeometryModelProps {
  modelId: string;
  color: string;
  convexHull: string | null;
  concaveHull: string | null;
  convexSansRaftHull: string | null;
  raftHeightMm: number;
  /** When true, use body-only geometry (supports rendered separately) */
  useBodyOnly: boolean;
}

function GeometryModel({
  modelId,
  color,
  convexHull,
  concaveHull,
  convexSansRaftHull,
  raftHeightMm,
  useBodyOnly,
}: GeometryModelProps) {
  const { data: fullData } = useGeometry(modelId);
  const { data: bodyData } = useBodyGeometry(modelId);

  // Use body-only geometry when supports are being rendered separately
  // and body geometry is available; otherwise fall back to full geometry.
  const data = useBodyOnly && bodyData != null ? bodyData : fullData;

  const bufferGeometry = useMemo(() => {
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.BufferAttribute(data.positions, 3));
    geo.setIndex(new THREE.BufferAttribute(data.indices, 1));
    geo.computeVertexNormals();
    return geo;
  }, [data]);

  const hullCoords = useMemo((): Array<[number, number]> | null => {
    if (!convexHull) return null;
    try {
      return JSON.parse(convexHull);
    } catch {
      return null;
    }
  }, [convexHull]);

  const sansRaftCoords = useMemo((): Array<[number, number]> | null => {
    if (!convexSansRaftHull) return null;
    try {
      return JSON.parse(convexSansRaftHull);
    } catch {
      return null;
    }
  }, [convexSansRaftHull]);

  const concaveCoords = useMemo((): Array<[number, number]> | null => {
    if (!concaveHull) return null;
    try {
      return JSON.parse(concaveHull);
    } catch {
      return null;
    }
  }, [concaveHull]);

  return (
    <group>
      <mesh geometry={bufferGeometry}>
        <meshStandardMaterial color={color} roughness={0.55} metalness={0.15} flatShading />
      </mesh>
      <HullPolygon coordinates={hullCoords} color="#818cf8" />
      <HullLine coordinates={hullCoords} color="#818cf8" />
      <HullPolygon coordinates={concaveCoords} color="#34d399" yOffset={0.02} opacity={0.22} />
      <HullLine coordinates={concaveCoords} color="#34d399" yOffset={0.02} />
      <HullPolygon
        coordinates={sansRaftCoords}
        color="#f59e0b"
        yOffset={raftHeightMm}
        opacity={0.18}
      />
      <HullLine coordinates={sansRaftCoords} color="#f59e0b" yOffset={raftHeightMm} />
    </group>
  );
}

interface ErrorBoundaryState {
  hasError: boolean;
}

interface SupportGeometryMeshProps {
  modelId: string;
  visible: boolean;
}

function SupportGeometryMesh({ modelId, visible }: SupportGeometryMeshProps) {
  const { data } = useSupportGeometry(modelId);

  const bufferGeometry = useMemo(() => {
    if (!data) return null;
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.BufferAttribute(data.positions, 3));
    geo.setIndex(new THREE.BufferAttribute(data.indices, 1));
    geo.computeVertexNormals();
    return geo;
  }, [data]);

  if (!data || !bufferGeometry) return null;

  return (
    <mesh geometry={bufferGeometry} visible={visible}>
      <meshStandardMaterial
        color="#f59e0b"
        roughness={0.55}
        metalness={0.1}
        transparent
        opacity={0.5}
        flatShading
        side={THREE.DoubleSide}
      />
    </mesh>
  );
}

class ViewerErrorBoundary extends React.Component<
  React.PropsWithChildren<{ fallback: React.ReactNode }>,
  ErrorBoundaryState
> {
  constructor(props: React.PropsWithChildren<{ fallback: React.ReactNode }>) {
    super(props);
    this.state = { hasError: false };
  }
  static getDerivedStateFromError(): ErrorBoundaryState {
    return { hasError: true };
  }
  render() {
    if (this.state.hasError) return this.props.fallback;
    return this.props.children;
  }
}

interface ModelViewerProps {
  modelId: string;
  convexHull?: string | null;
  concaveHull?: string | null;
  convexSansRaftHull?: string | null;
  supported?: boolean | null;
}

export default function ModelViewer({
  modelId,
  convexHull,
  concaveHull,
  convexSansRaftHull,
  supported,
}: ModelViewerProps) {
  const { data: model, isPending, isError } = useModel(modelId);
  const { data: supportData } = useSupportGeometry(modelId);
  const [showSupports, setShowSupports] = useState(true);
  const color = MODEL_COLOR;

  const hasSupportMesh = supported === true && supportData != null;

  const errorFallback = (
    <div style={containerStyle}>
      <span style={{ fontSize: '2rem', opacity: 0.3 }}>⬡</span>
      <span style={{ fontSize: '0.85rem', color: '#64748b' }}>Could not load 3D model</span>
    </div>
  );

  if (isError) return errorFallback;

  if (
    isPending ||
    model.dimensionXMm == null ||
    model.dimensionYMm == null ||
    model.dimensionZMm == null
  ) {
    return (
      <div
        style={{
          ...containerStyle,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
        }}
      >
        <span style={{ fontSize: '0.85rem', color: '#64748b' }}>
          {isPending ? 'Loading model…' : 'Model dimensions not yet available'}
        </span>
      </div>
    );
  }

  const orbitTarget = useMemo<[number, number, number]>(
    () => [model.sphereCentreX ?? 0, model.sphereCentreY ?? 0, model.sphereCentreZ ?? 0],
    [model.dimensionYMm, model.sphereCentreX, model.sphereCentreY, model.sphereCentreZ],
  );

  const halfExtents = useMemo(
    () =>
      new THREE.Vector3(model.dimensionXMm! / 2, model.dimensionYMm! / 2, model.dimensionZMm! / 2),
    [model.dimensionXMm, model.dimensionYMm, model.dimensionZMm],
  );

  return (
    <ViewerErrorBoundary fallback={errorFallback}>
      <div style={wrapperStyle}>
        <Canvas camera={{ fov: 45 }} gl={{ antialias: true }} style={containerStyle}>
          <CameraInit
            target={orbitTarget}
            halfExtents={halfExtents}
            direction={DEFAULT_VIEW_DIRECTION}
          />
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
            <GeometryModel
              modelId={modelId}
              color={color}
              convexHull={convexHull ?? null}
              concaveHull={concaveHull ?? null}
              convexSansRaftHull={convexSansRaftHull ?? null}
              raftHeightMm={model.raftHeightMm}
              useBodyOnly={hasSupportMesh}
            />
            {supported === true && <SupportGeometryMesh modelId={modelId} visible={showSupports} />}
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
        {hasSupportMesh && (
          <button
            style={{
              ...toggleButtonStyle,
              background: showSupports ? 'rgba(245,158,11,0.25)' : 'rgba(255,255,255,0.07)',
              color: showSupports ? '#f59e0b' : '#64748b',
              borderColor: showSupports ? 'rgba(245,158,11,0.5)' : 'rgba(255,255,255,0.12)',
            }}
            onClick={() => setShowSupports((v) => !v)}
            title={showSupports ? 'Hide supports' : 'Show supports'}
          >
            {showSupports ? 'Supports: On' : 'Supports: Off'}
          </button>
        )}
      </div>
    </ViewerErrorBoundary>
  );
}

const wrapperStyle: React.CSSProperties = {
  position: 'relative',
  width: '100%',
  height: '100%',
};

const containerStyle: React.CSSProperties = {
  width: '100%',
  height: '100%',
  background: '#0f172a',
};

const toggleButtonStyle: React.CSSProperties = {
  position: 'absolute',
  bottom: 10,
  right: 10,
  padding: '4px 10px',
  border: '1px solid',
  borderRadius: 6,
  cursor: 'pointer',
  fontSize: '0.75rem',
  fontWeight: 500,
  letterSpacing: '0.05em',
  transition: 'background 0.15s, color 0.15s',
};
