import React, { useEffect, useMemo } from 'react';
import { useTheme } from '@mui/material';
import { Canvas, useThree } from '@react-three/fiber';
import { Line, OrbitControls } from '@react-three/drei';
import * as THREE from 'three';
import { useGeometry, useModel, useSplitGeometry } from '../lib/queries';
import { useRenderControls } from './RenderControlsContext';

const DEFAULT_VIEW_DIRECTION = new THREE.Vector3(1, 0.8, -1).normalize();
const FRAMING_PADDING = 1.15;

const MODEL_COLOR = '#818cf8';

function SceneBg({ color }: { color: string }) {
  return <color attach="background" args={[color]} />;
}

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
  geometry: import('../lib/api').GeometryResponse;
  color: string;
  convexHull: string | null;
  concaveHull: string | null;
  convexSansRaftHull: string | null;
  raftHeightMm: number;
}

function GeometryModel({
  geometry: data,
  color,
  convexHull,
  concaveHull,
  convexSansRaftHull,
  raftHeightMm,
}: GeometryModelProps) {
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

interface SupportForceArrowsProps {
  points: import('../lib/api').AutoSupportPoint[] | null;
  visible: boolean;
}

const SIZE_COLORS: Record<string, [string, string]> = {
  micro: ['#93c5fd', '#3b82f6'],
  light: ['#fbbf24', '#f59e0b'],
  medium: ['#fb923c', '#ea580c'],
  heavy: ['#ef4444', '#b91c1c'],
};

function SupportForceArrow({
  point,
  maxMagnitude,
}: {
  point: import('../lib/api').AutoSupportPoint;
  maxMagnitude: number;
}) {
  const { arrowData, sphereColor } = useMemo(() => {
    const force = new THREE.Vector3(point.pullForce.x, point.pullForce.y, point.pullForce.z);
    const magnitude = force.length();
    const [lowColor, highColor] = SIZE_COLORS[point.size] ?? SIZE_COLORS.medium;
    const normalizedMagnitude = maxMagnitude <= 0 ? 0 : magnitude / maxMagnitude;
    const color = new THREE.Color().lerpColors(
      new THREE.Color(lowColor),
      new THREE.Color(highColor),
      normalizedMagnitude,
    );

    if (magnitude < 0.001) return { arrowData: null, sphereColor: color };

    const start = new THREE.Vector3(point.x, point.y + point.radiusMm * 0.25, point.z);
    const direction = force.normalize();
    const length = Math.max(point.radiusMm * 2.2, Math.min(magnitude * 1.2, 18));
    const headLength = Math.max(point.radiusMm * 1.4, Math.min(length * 0.28, 3));
    const shaftEnd = start
      .clone()
      .addScaledVector(direction, Math.max(length - headLength, point.radiusMm));
    const coneCenter = shaftEnd.clone().addScaledVector(direction, headLength * 0.5);
    const coneQuaternion = new THREE.Quaternion().setFromUnitVectors(
      new THREE.Vector3(0, 1, 0),
      direction,
    );
    return {
      arrowData: { coneCenter, coneQuaternion, color, headLength, linePoints: [start, shaftEnd] },
      sphereColor: color,
    };
  }, [maxMagnitude, point]);

  return (
    <group>
      <mesh position={[point.x, point.y, point.z]}>
        <sphereGeometry args={[point.radiusMm, 12, 8]} />
        <meshStandardMaterial color={sphereColor} roughness={0.4} metalness={0.1} />
      </mesh>
      {arrowData && (
        <>
          <Line points={arrowData.linePoints} color={arrowData.color} lineWidth={1.5} />
          <mesh position={arrowData.coneCenter} quaternion={arrowData.coneQuaternion}>
            <coneGeometry
              args={[Math.max(point.radiusMm * 0.55, 0.25), arrowData.headLength, 10]}
            />
            <meshBasicMaterial color={arrowData.color} />
          </mesh>
        </>
      )}
    </group>
  );
}

function SupportForceArrows({ points, visible }: SupportForceArrowsProps) {
  const maxMagnitude = useMemo(
    () =>
      points?.reduce(
        (max, point) =>
          Math.max(max, Math.hypot(point.pullForce.x, point.pullForce.y, point.pullForce.z)),
        0,
      ) ?? 0,
    [points],
  );

  if (!visible || !points || points.length === 0) return null;

  return (
    <group>
      {points.map((point, index) => (
        <SupportForceArrow
          key={`${point.x}-${point.y}-${point.z}-${index}`}
          point={point}
          maxMagnitude={maxMagnitude}
        />
      ))}
    </group>
  );
}

interface ErrorBoundaryState {
  hasError: boolean;
}

interface SupportGeometryMeshProps {
  geometry: import('../lib/api').GeometryResponse | null;
  visible: boolean;
}

function SupportGeometryMesh({ geometry: data, visible }: SupportGeometryMeshProps) {
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

interface IslandHighlightsProps {
  islands: import('../lib/api').AutoSupportIsland[] | null;
  visible: boolean;
}

function IslandHighlights({ islands, visible }: IslandHighlightsProps) {
  if (!visible || !islands || islands.length === 0) return null;

  return (
    <group>
      {islands.map((island, index) => {
        const radius = Math.max(island.radiusMm, 0.3);
        return (
          <group
            key={`island-${island.centroidX}-${island.centroidZ}-${island.sliceHeightMm}-${index}`}
            position={[island.centroidX, island.sliceHeightMm, island.centroidZ]}
          >
            <mesh rotation={[-Math.PI / 2, 0, 0]}>
              <ringGeometry args={[radius * 0.85, radius, 24]} />
              <meshBasicMaterial
                color="#ef4444"
                transparent
                opacity={0.6}
                side={THREE.DoubleSide}
              />
            </mesh>
            <mesh rotation={[-Math.PI / 2, 0, 0]}>
              <circleGeometry args={[radius, 24]} />
              <meshBasicMaterial
                color="#ef4444"
                transparent
                opacity={0.15}
                side={THREE.DoubleSide}
              />
            </mesh>
          </group>
        );
      })}
    </group>
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
  splitGeometryOverride?: import('../lib/api').SplitGeometryResponse | null;
  supportPointsOverride?: import('../lib/api').AutoSupportPoint[] | null;
  islandsOverride?: import('../lib/api').AutoSupportIsland[] | null;
  showForceMarkers?: boolean;
}

export default function ModelViewer({
  modelId,
  convexHull,
  concaveHull,
  convexSansRaftHull,
  supported,
  splitGeometryOverride,
  supportPointsOverride,
  islandsOverride,
  showForceMarkers = true,
}: ModelViewerProps) {
  const { data: model, isPending, isError } = useModel(modelId);
  const { showSupports, setSupportsToggleAvailable } = useRenderControls();
  const hasSplitOverride = splitGeometryOverride !== undefined;
  const shouldFetchStoredSplitGeometry = supported === true && !hasSplitOverride;
  const { data: splitData, isPending: isSplitPending } = useSplitGeometry(
    modelId,
    shouldFetchStoredSplitGeometry,
  );
  const activeSplitData = hasSplitOverride ? (splitGeometryOverride ?? null) : splitData;
  const shouldFetchFullGeometry =
    !hasSplitOverride && (supported !== true || activeSplitData === null);
  const { data: fullData, isPending: isFullGeometryPending } = useGeometry(
    modelId,
    shouldFetchFullGeometry,
  );
  const color = MODEL_COLOR;
  const theme = useTheme();
  const geometryData = activeSplitData != null ? activeSplitData.body : fullData;
  const orbitTarget = useMemo<[number, number, number]>(
    () => [model?.sphereCentreX ?? 0, model?.sphereCentreY ?? 0, model?.sphereCentreZ ?? 0],
    [model?.sphereCentreX, model?.sphereCentreY, model?.sphereCentreZ],
  );
  const halfExtents = useMemo(
    () =>
      new THREE.Vector3(
        (model?.dimensionXMm ?? 0) / 2,
        (model?.dimensionYMm ?? 0) / 2,
        (model?.dimensionZMm ?? 0) / 2,
      ),
    [model?.dimensionXMm, model?.dimensionYMm, model?.dimensionZMm],
  );

  const hasSupportMesh = activeSplitData?.supports != null;
  const shouldShowForceMarkers = showSupports && showForceMarkers;

  useEffect(() => {
    setSupportsToggleAvailable(hasSupportMesh);
    return () => setSupportsToggleAvailable(false);
  }, [hasSupportMesh, setSupportsToggleAvailable]);

  const isGeometryPending = hasSplitOverride
    ? false
    : supported === true
      ? isSplitPending || (activeSplitData === null && isFullGeometryPending)
      : isFullGeometryPending;

  const errorFallback = (
    <div style={containerStyle}>
      <span style={{ fontSize: '2rem', opacity: 0.3 }}>⬡</span>
      <span style={{ fontSize: '0.85rem', color: '#64748b' }}>Could not load 3D model</span>
    </div>
  );

  if (isError) return errorFallback;

  if (
    isPending ||
    isGeometryPending ||
    geometryData == null ||
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
          {isPending || isGeometryPending || geometryData == null
            ? 'Loading model…'
            : 'Model dimensions not yet available'}
        </span>
      </div>
    );
  }

  return (
    <ViewerErrorBoundary fallback={errorFallback}>
      <div style={wrapperStyle}>
        <Canvas camera={{ fov: 45 }} gl={{ antialias: true }} style={containerStyle}>
          <SceneBg color={theme.palette.background.default} />
          <CameraInit
            target={orbitTarget}
            halfExtents={halfExtents}
            direction={DEFAULT_VIEW_DIRECTION}
          />
          <Lighting />
          <Grid />
          <GeometryModel
            geometry={geometryData}
            color={color}
            convexHull={convexHull ?? null}
            concaveHull={concaveHull ?? null}
            convexSansRaftHull={convexSansRaftHull ?? null}
            raftHeightMm={model.raftHeightMm}
          />
          {hasSupportMesh && (
            <SupportGeometryMesh
              geometry={activeSplitData?.supports ?? null}
              visible={showSupports}
            />
          )}
          <SupportForceArrows
            points={supportPointsOverride ?? null}
            visible={shouldShowForceMarkers}
          />
          <IslandHighlights islands={islandsOverride ?? null} visible={shouldShowForceMarkers} />
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
  background: 'var(--color-bg-default)',
};
