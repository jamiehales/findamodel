import React, { useEffect, useMemo, useState } from 'react';
import { useTheme } from '@mui/material';
import { Canvas, useThree } from '@react-three/fiber';
import { Line, OrbitControls } from '@react-three/drei';
import * as THREE from 'three';
import { useGeometry, useModel, useSplitGeometry } from '../lib/queries';
import { useRenderControls } from './RenderControlsContext';

const DEFAULT_VIEW_DIRECTION = new THREE.Vector3(1, 0.8, -1).normalize();
const FRAMING_PADDING = 1.15;

const MODEL_COLOR = '#818cf8';
const sliceMaskTextureCache = new Map<string, THREE.Texture>();

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
  selectedSliceClipHeightMm?: number | null;
}

function GeometryModel({
  geometry: data,
  color,
  convexHull,
  concaveHull,
  convexSansRaftHull,
  raftHeightMm,
  selectedSliceClipHeightMm,
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

  const clippingPlanes = useMemo(() => {
    if (selectedSliceClipHeightMm == null) return [];
    return [new THREE.Plane(new THREE.Vector3(0, -1, 0), selectedSliceClipHeightMm)];
  }, [selectedSliceClipHeightMm]);

  return (
    <group>
      <mesh geometry={bufferGeometry}>
        <meshStandardMaterial
          color={color}
          roughness={0.55}
          metalness={0.15}
          flatShading
          clippingPlanes={clippingPlanes}
          clipShadows
        />
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
  selectedSliceHeightMm?: number | null;
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
  selectedSliceHeightMm,
  maxMagnitude,
}: {
  point: import('../lib/api').AutoSupportPoint;
  selectedSliceHeightMm?: number | null;
  maxMagnitude: number;
}) {
  const { arrowData, peelArrowData, rotationArrowData, gravityArrowData, sphereColor } =
    useMemo(() => {
      const selectedLayerForce =
        selectedSliceHeightMm == null || !point.layerForces || point.layerForces.length === 0
          ? null
          : point.layerForces.reduce((best, sample) => {
              if (!best) return sample;
              const bestDistance = Math.abs(best.sliceHeightMm - selectedSliceHeightMm);
              const sampleDistance = Math.abs(sample.sliceHeightMm - selectedSliceHeightMm);
              return sampleDistance < bestDistance ? sample : best;
            }, point.layerForces[0]);

      const activeForce = selectedLayerForce?.total ?? point.pullForce;
      const force = new THREE.Vector3(activeForce.x, activeForce.y, activeForce.z);
      const magnitude = force.length();
      const [lowColor, highColor] = SIZE_COLORS[point.size] ?? SIZE_COLORS.medium;
      const normalizedMagnitude = maxMagnitude <= 0 ? 0 : magnitude / maxMagnitude;
      const color = new THREE.Color().lerpColors(
        new THREE.Color(lowColor),
        new THREE.Color(highColor),
        normalizedMagnitude,
      );

      const buildArrow = (vector: THREE.Vector3, arrowColor: THREE.Color) => {
        const vectorMagnitude = vector.length();
        if (vectorMagnitude < 0.001) return null;

        const start = new THREE.Vector3(point.x, point.y + point.radiusMm * 0.25, point.z);
        const direction = vector.clone().normalize();
        const length = Math.max(point.radiusMm * 2.2, Math.min(vectorMagnitude * 1.2, 18));
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
          coneCenter,
          coneQuaternion,
          color: arrowColor,
          headLength,
          linePoints: [start, shaftEnd],
        };
      };

      const totalArrow = buildArrow(force, color);
      const peelArrow = selectedLayerForce
        ? buildArrow(
            new THREE.Vector3(
              selectedLayerForce.peel.x,
              selectedLayerForce.peel.y,
              selectedLayerForce.peel.z,
            ),
            new THREE.Color('#f59e0b'),
          )
        : null;
      const rotationArrow = selectedLayerForce
        ? buildArrow(
            new THREE.Vector3(
              selectedLayerForce.rotation.x,
              selectedLayerForce.rotation.y,
              selectedLayerForce.rotation.z,
            ),
            new THREE.Color('#ef4444'),
          )
        : null;
      const gravityArrow = selectedLayerForce
        ? buildArrow(
            new THREE.Vector3(
              selectedLayerForce.gravity.x,
              selectedLayerForce.gravity.y,
              selectedLayerForce.gravity.z,
            ),
            new THREE.Color('#3b82f6'),
          )
        : null;

      return {
        arrowData: totalArrow,
        peelArrowData: peelArrow,
        rotationArrowData: rotationArrow,
        gravityArrowData: gravityArrow,
        sphereColor: color,
      };
    }, [maxMagnitude, point, selectedSliceHeightMm]);

  const renderArrow = (
    data: {
      coneCenter: THREE.Vector3;
      coneQuaternion: THREE.Quaternion;
      color: THREE.Color;
      headLength: number;
      linePoints: THREE.Vector3[];
    } | null,
  ) => {
    if (!data) return null;
    return (
      <>
        <Line points={data.linePoints} color={data.color} lineWidth={1.5} />
        <mesh position={data.coneCenter} quaternion={data.coneQuaternion}>
          <coneGeometry args={[Math.max(point.radiusMm * 0.55, 0.25), data.headLength, 10]} />
          <meshBasicMaterial color={data.color} />
        </mesh>
      </>
    );
  };

  return (
    <group>
      <mesh position={[point.x, point.y, point.z]}>
        <sphereGeometry args={[point.radiusMm, 12, 8]} />
        <meshStandardMaterial color={sphereColor} roughness={0.4} metalness={0.1} />
      </mesh>
      {renderArrow(gravityArrowData)}
      {renderArrow(peelArrowData)}
      {renderArrow(rotationArrowData)}
      {renderArrow(arrowData)}
    </group>
  );
}

function SupportForceArrows({ points, selectedSliceHeightMm, visible }: SupportForceArrowsProps) {
  const visiblePoints = useMemo(() => {
    if (!points) return null;
    if (selectedSliceHeightMm == null) return points;

    return points.filter((point) => {
      if (point.layerForces && point.layerForces.length > 0) {
        return point.layerForces.some(
          (layer) => layer.sliceHeightMm <= selectedSliceHeightMm + 1e-3,
        );
      }

      return point.y <= selectedSliceHeightMm + 1e-3;
    });
  }, [points, selectedSliceHeightMm]);

  const maxMagnitude = useMemo(
    () =>
      visiblePoints?.reduce(
        (max, point) =>
          Math.max(max, Math.hypot(point.pullForce.x, point.pullForce.y, point.pullForce.z)),
        0,
      ) ?? 0,
    [visiblePoints],
  );

  if (!visible || !visiblePoints || visiblePoints.length === 0) return null;

  return (
    <group>
      {visiblePoints.map((point, index) => (
        <SupportForceArrow
          key={`${point.x}-${point.y}-${point.z}-${index}`}
          point={point}
          selectedSliceHeightMm={selectedSliceHeightMm}
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
  selectedSliceClipHeightMm?: number | null;
}

function SupportGeometryMesh({
  geometry: data,
  visible,
  selectedSliceClipHeightMm,
}: SupportGeometryMeshProps) {
  const bufferGeometry = useMemo(() => {
    if (!data) return null;
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.BufferAttribute(data.positions, 3));
    geo.setIndex(new THREE.BufferAttribute(data.indices, 1));
    geo.computeVertexNormals();
    return geo;
  }, [data]);

  const clippingPlanes = useMemo(() => {
    if (selectedSliceClipHeightMm == null) return [];
    return [new THREE.Plane(new THREE.Vector3(0, -1, 0), selectedSliceClipHeightMm)];
  }, [selectedSliceClipHeightMm]);

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
        clippingPlanes={clippingPlanes}
        clipShadows
      />
    </mesh>
  );
}

interface IslandHighlightsProps {
  islands: import('../lib/api').AutoSupportIsland[] | null;
  selectedSliceHeightMm?: number | null;
  visible: boolean;
}

function IslandHighlights({ islands, selectedSliceHeightMm, visible }: IslandHighlightsProps) {
  const visibleIslands = useMemo(() => {
    if (!islands) return null;
    if (selectedSliceHeightMm == null) return islands;
    return islands.filter(
      (island) => Math.abs(island.sliceHeightMm - selectedSliceHeightMm) <= 0.001,
    );
  }, [islands, selectedSliceHeightMm]);

  if (!visible || !visibleIslands || visibleIslands.length === 0) return null;

  return (
    <group>
      {visibleIslands.map((island, index) => {
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

function SliceLayerOverlay({
  islands,
  sliceLayers,
  selectedSliceLayerIndex,
  selectedSliceHeightMm,
}: {
  islands: import('../lib/api').AutoSupportIsland[] | null;
  sliceLayers?: import('../lib/api').AutoSupportSliceLayer[] | null;
  selectedSliceLayerIndex?: number | null;
  selectedSliceHeightMm?: number | null;
}) {
  if (selectedSliceHeightMm == null) return null;

  const selectedSliceLayer =
    sliceLayers &&
    selectedSliceLayerIndex != null &&
    selectedSliceLayerIndex >= 0 &&
    selectedSliceLayerIndex < sliceLayers.length
      ? sliceLayers[selectedSliceLayerIndex]
      : null;

  if (selectedSliceLayer?.sliceMaskPngBase64) {
    return (
      <SliceMaskPlane layer={selectedSliceLayer} selectedSliceHeightMm={selectedSliceHeightMm} />
    );
  }

  if (!islands || islands.length === 0) return null;

  const layerIslands = islands.filter(
    (island) =>
      Math.abs(island.sliceHeightMm - selectedSliceHeightMm) <= 0.001 &&
      island.boundary &&
      island.boundary.length >= 3,
  );

  if (layerIslands.length === 0) return null;

  return (
    <group>
      {layerIslands.map((island, index) => {
        const coordinates = (island.boundary ?? []).map(
          (vertex) => [vertex.x, vertex.z] as [number, number],
        );
        return (
          <group key={`slice-layer-${index}`}>
            <HullPolygon
              coordinates={coordinates}
              color="#22d3ee"
              yOffset={selectedSliceHeightMm}
              opacity={0.28}
            />
            <HullLine coordinates={coordinates} color="#06b6d4" yOffset={selectedSliceHeightMm} />
          </group>
        );
      })}
    </group>
  );
}

function SliceMaskPlane({
  layer,
  selectedSliceHeightMm,
}: {
  layer: import('../lib/api').AutoSupportSliceLayer;
  selectedSliceHeightMm: number;
}) {
  const [texture, setTexture] = useState<THREE.Texture | null>(null);
  const sliceMaskUrl = useMemo(
    () => (layer.sliceMaskPngBase64 ? `data:image/png;base64,${layer.sliceMaskPngBase64}` : null),
    [layer.sliceMaskPngBase64],
  );

  useEffect(() => {
    if (!sliceMaskUrl) {
      setTexture(null);
      return;
    }

    const cachedTexture = sliceMaskTextureCache.get(sliceMaskUrl);
    if (cachedTexture) {
      setTexture(cachedTexture);
      return;
    }

    let cancelled = false;
    const loader = new THREE.TextureLoader();
    loader.load(sliceMaskUrl, (loadedTexture) => {
      if (cancelled) return;

      loadedTexture.minFilter = THREE.NearestFilter;
      loadedTexture.magFilter = THREE.NearestFilter;
      loadedTexture.wrapS = THREE.ClampToEdgeWrapping;
      loadedTexture.wrapT = THREE.ClampToEdgeWrapping;
      // Slice bitmaps are encoded with row 0 as +Z, so disable default Y flip.
      loadedTexture.flipY = false;
      loadedTexture.needsUpdate = true;
      sliceMaskTextureCache.set(sliceMaskUrl, loadedTexture);
      setTexture(loadedTexture);
    });

    return () => {
      cancelled = true;
    };
  }, [sliceMaskUrl]);

  if (!texture) return null;

  return (
    <mesh position={[0, selectedSliceHeightMm + 0.02, 0]} rotation={[-Math.PI / 2, 0, 0]}>
      <planeGeometry args={[layer.bedWidthMm, layer.bedDepthMm]} />
      <meshBasicMaterial
        map={texture}
        transparent
        opacity={0.75}
        depthWrite={false}
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
  modelOverride?: {
    sphereCentreX: number;
    sphereCentreY: number;
    sphereCentreZ: number;
    dimensionXMm: number;
    dimensionYMm: number;
    dimensionZMm: number;
    raftHeightMm: number;
  } | null;
  convexHull?: string | null;
  concaveHull?: string | null;
  convexSansRaftHull?: string | null;
  supported?: boolean | null;
  splitGeometryOverride?: import('../lib/api').SplitGeometryResponse | null;
  supportPointsOverride?: import('../lib/api').AutoSupportPoint[] | null;
  islandsOverride?: import('../lib/api').AutoSupportIsland[] | null;
  sliceLayersOverride?: import('../lib/api').AutoSupportSliceLayer[] | null;
  selectedSliceLayerIndex?: number | null;
  selectedSliceHeightMm?: number | null;
  slicePreviewEnabled?: boolean;
  showForceMarkers?: boolean;
}

export default function ModelViewer({
  modelId,
  modelOverride,
  convexHull,
  concaveHull,
  convexSansRaftHull,
  supported,
  splitGeometryOverride,
  supportPointsOverride,
  islandsOverride,
  sliceLayersOverride,
  selectedSliceLayerIndex,
  selectedSliceHeightMm,
  slicePreviewEnabled = false,
  showForceMarkers = true,
}: ModelViewerProps) {
  const shouldFetchModel = modelOverride == null;
  const { data: fetchedModel, isPending, isError } = useModel(modelId, shouldFetchModel);
  const model = modelOverride ?? fetchedModel;
  const isModelPending = shouldFetchModel && isPending;
  const hasModelError = shouldFetchModel && isError;
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

  const geometryBounds = useMemo(() => {
    if (!geometryData || geometryData.positions.length < 3) return null;

    const positions = geometryData.positions;
    let minX = Number.POSITIVE_INFINITY;
    let minY = Number.POSITIVE_INFINITY;
    let minZ = Number.POSITIVE_INFINITY;
    let maxX = Number.NEGATIVE_INFINITY;
    let maxY = Number.NEGATIVE_INFINITY;
    let maxZ = Number.NEGATIVE_INFINITY;

    for (let i = 0; i < positions.length; i += 3) {
      const x = positions[i];
      const y = positions[i + 1];
      const z = positions[i + 2];
      if (x < minX) minX = x;
      if (y < minY) minY = y;
      if (z < minZ) minZ = z;
      if (x > maxX) maxX = x;
      if (y > maxY) maxY = y;
      if (z > maxZ) maxZ = z;
    }

    return {
      centre: {
        x: (minX + maxX) * 0.5,
        y: (minY + maxY) * 0.5,
        z: (minZ + maxZ) * 0.5,
      },
      dimensionX: Math.max(maxX - minX, 0.001),
      dimensionY: Math.max(maxY - minY, 0.001),
      dimensionZ: Math.max(maxZ - minZ, 0.001),
    };
  }, [geometryData]);

  const frameSphereCentre = geometryBounds?.centre ?? geometryData?.sphereCentre ?? null;
  const frameDimensionX =
    geometryBounds?.dimensionX ?? geometryData?.dimensionXMm ?? model?.dimensionXMm ?? 0;
  const frameDimensionY =
    geometryBounds?.dimensionY ?? geometryData?.dimensionYMm ?? model?.dimensionYMm ?? 0;
  const frameDimensionZ =
    geometryBounds?.dimensionZ ?? geometryData?.dimensionZMm ?? model?.dimensionZMm ?? 0;

  const orbitTarget = useMemo<[number, number, number]>(
    () => [
      frameSphereCentre?.x ?? model?.sphereCentreX ?? 0,
      frameSphereCentre?.y ?? model?.sphereCentreY ?? 0,
      frameSphereCentre?.z ?? model?.sphereCentreZ ?? 0,
    ],
    [
      frameSphereCentre?.x,
      frameSphereCentre?.y,
      frameSphereCentre?.z,
      model?.sphereCentreX,
      model?.sphereCentreY,
      model?.sphereCentreZ,
    ],
  );
  const halfExtents = useMemo(
    () => new THREE.Vector3(frameDimensionX / 2, frameDimensionY / 2, frameDimensionZ / 2),
    [frameDimensionX, frameDimensionY, frameDimensionZ],
  );

  const hasSupportMesh = activeSplitData?.supports != null;
  const shouldShowForceMarkers = showSupports && showForceMarkers;
  const effectiveSliceHeightMm = slicePreviewEnabled ? selectedSliceHeightMm : null;

  const effectiveSliceClipHeightMm = useMemo(() => {
    if (!slicePreviewEnabled || effectiveSliceHeightMm == null) return null;

    const layers = sliceLayersOverride ?? [];
    if (selectedSliceLayerIndex == null || layers.length <= 1) {
      return effectiveSliceHeightMm;
    }

    const currentLayer = layers[selectedSliceLayerIndex];
    if (!currentLayer) return effectiveSliceHeightMm;

    const referenceLayer =
      selectedSliceLayerIndex + 1 < layers.length
        ? layers[selectedSliceLayerIndex + 1]
        : selectedSliceLayerIndex > 0
          ? layers[selectedSliceLayerIndex - 1]
          : null;
    if (!referenceLayer) return effectiveSliceHeightMm;

    const layerHeightMm = Math.abs(referenceLayer.sliceHeightMm - currentLayer.sliceHeightMm);
    if (layerHeightMm <= 0) return effectiveSliceHeightMm;

    return effectiveSliceHeightMm + layerHeightMm * 0.5;
  }, [slicePreviewEnabled, effectiveSliceHeightMm, selectedSliceLayerIndex, sliceLayersOverride]);

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

  if (hasModelError) return errorFallback;

  if (
    isModelPending ||
    isGeometryPending ||
    model == null ||
    geometryData == null ||
    model?.dimensionXMm == null ||
    model?.dimensionYMm == null ||
    model?.dimensionZMm == null
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
          {isModelPending || isGeometryPending || geometryData == null
            ? 'Loading model…'
            : 'Model dimensions not yet available'}
        </span>
      </div>
    );
  }

  return (
    <ViewerErrorBoundary fallback={errorFallback}>
      <div style={wrapperStyle}>
        <Canvas
          camera={{ fov: 45 }}
          gl={{ antialias: true, localClippingEnabled: true }}
          style={containerStyle}
        >
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
            selectedSliceClipHeightMm={effectiveSliceClipHeightMm}
          />
          {hasSupportMesh && (
            <SupportGeometryMesh
              geometry={activeSplitData?.supports ?? null}
              visible={showSupports}
              selectedSliceClipHeightMm={effectiveSliceClipHeightMm}
            />
          )}
          <SupportForceArrows
            points={supportPointsOverride ?? null}
            selectedSliceHeightMm={effectiveSliceHeightMm}
            visible={shouldShowForceMarkers}
          />
          {slicePreviewEnabled && (
            <SliceLayerOverlay
              islands={islandsOverride ?? null}
              sliceLayers={sliceLayersOverride ?? null}
              selectedSliceLayerIndex={selectedSliceLayerIndex}
              selectedSliceHeightMm={effectiveSliceHeightMm}
            />
          )}
          <IslandHighlights
            islands={islandsOverride ?? null}
            selectedSliceHeightMm={effectiveSliceHeightMm}
            visible={shouldShowForceMarkers}
          />
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
