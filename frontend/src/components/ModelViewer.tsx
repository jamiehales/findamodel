import React, { useEffect, useMemo, useState } from 'react';
import { Canvas, useThree } from '@react-three/fiber';
import { OrbitControls } from '@react-three/drei';
import * as THREE from 'three';
import {
  Checkbox,
  SegmentedControl,
  SegmentedControlItem,
  Spinner,
} from '@jamiehales/ui-kit/mostlymaking';
import {
  useGeometry,
  useModel,
  useRepairedGeometry,
  useResupportedGeometry,
  useSpinMesh,
  useSplitGeometry,
} from '../lib/queries';
import type { Model } from '../lib/api';
import { createPreviewMaterial } from '../lib/previewShading';
import type { SpinMesh } from '../lib/spinMesh';
import styles from './ModelViewer.module.css';

const DEFAULT_VIEW_DIRECTION = new THREE.Vector3(1, 0.8, -1).normalize();
const FRAMING_PADDING = 1.15;

// Colors from the mostlymaking data palette. Keep MODEL_COLOR and SUPPORT_COLOR in
// step with BODY_COLOR and SUPPORT_COLOR in backend/src/preview/mod.rs.
const MODEL_COLOR = '#a8404f';
const SUPPORT_COLOR = '#c9a45c';
const CONVEX_HULL_COLOR = '#b892c0';
const CONCAVE_HULL_COLOR = '#8fb894';
const SANS_RAFT_HULL_COLOR = '#94a7cc';
const SCENE_BACKGROUND = '#18171a';

/** The `--slate` design token (accent for the faces a repair made), read once from the page. */
function repairedFaceColor(): string {
  const token =
    typeof document === 'undefined'
      ? ''
      : getComputedStyle(document.documentElement).getPropertyValue('--slate').trim();
  return /^#[0-9a-f]{6}$/i.test(token) ? token : SANS_RAFT_HULL_COLOR;
}

type SupportMode = 'supported' | 'resupported' | 'unsupported';

function SceneBg({ color }: { color: string }) {
  return <color attach="background" args={[color]} />;
}

function Grid() {
  return <gridHelper args={[25.4 * 2, 2, '#3d3943', '#2a282e']} position={[0, 0, 0]} />;
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
  /** One flag per triangle: drawn in the repaired-face accent when set. */
  changed?: Uint8Array | null;
  color: string;
}

interface HullOverlaysProps {
  convexHull: string | null;
  concaveHull: string | null;
  convexSansRaftHull: string | null;
  raftHeightMm: number;
}

/** Material shaded like the rendered previews, disposed when it changes or unmounts. */
function usePreviewMaterial(color: string, opacity?: number): THREE.ShaderMaterial {
  const material = useMemo(() => createPreviewMaterial(color, opacity), [color, opacity]);
  useEffect(() => () => material.dispose(), [material]);
  return material;
}

/** The mesh's triangles, those flagged in `changed` last and in a second draw group. */
function buildBodyGeometry(
  data: import('../lib/api').GeometryResponse,
  changed: Uint8Array | null | undefined,
): THREE.BufferGeometry {
  const geo = new THREE.BufferGeometry();
  geo.setAttribute('position', new THREE.BufferAttribute(data.positions, 3));
  if (!changed) {
    geo.setIndex(new THREE.BufferAttribute(data.indices, 1));
    return geo;
  }
  const source = data.indices;
  const sorted = new (source.constructor as Uint16ArrayConstructor | Uint32ArrayConstructor)(
    source.length,
  );
  let changedCount = 0;
  for (let t = 0; t < data.triangleCount; t++) changedCount += changed[t];
  const kept = (data.triangleCount - changedCount) * 3;
  let keptAt = 0;
  let changedAt = kept;
  for (let t = 0; t < data.triangleCount; t++) {
    const triangle = source.subarray(t * 3, t * 3 + 3);
    if (changed[t]) {
      sorted.set(triangle, changedAt);
      changedAt += 3;
    } else {
      sorted.set(triangle, keptAt);
      keptAt += 3;
    }
  }
  geo.setIndex(new THREE.BufferAttribute(sorted, 1));
  geo.addGroup(0, kept, 0);
  geo.addGroup(kept, sorted.length - kept, 1);
  return geo;
}

function GeometryModel({ geometry: data, changed, color }: GeometryModelProps) {
  const bufferGeometry = useMemo(() => buildBodyGeometry(data, changed), [data, changed]);
  useEffect(() => () => bufferGeometry.dispose(), [bufferGeometry]);
  const bodyMaterial = usePreviewMaterial(color);
  const changedColor = useMemo(repairedFaceColor, []);
  const changedMaterial = usePreviewMaterial(changedColor);
  const material = useMemo(
    () => (changed ? [bodyMaterial, changedMaterial] : bodyMaterial),
    [changed, bodyMaterial, changedMaterial],
  );

  return <mesh geometry={bufferGeometry} material={material} />;
}

/**
 * The reduced body (the tile spin mesh) shown while the full mesh downloads. Its positions are
 * already in the full mesh's frame, so it sits where the full body will appear. The geometry is
 * disposed when it unmounts (once the full mesh replaces it); the cached arrays are left alone.
 */
function SpinPreviewModel({ spin, color }: { spin: SpinMesh; color: string }) {
  const bufferGeometry = useMemo(() => {
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.BufferAttribute(spin.positions, 3));
    geo.setIndex(new THREE.BufferAttribute(spin.indices, 1));
    return geo;
  }, [spin]);
  useEffect(() => () => bufferGeometry.dispose(), [bufferGeometry]);
  const material = usePreviewMaterial(color);
  return <mesh geometry={bufferGeometry} material={material} />;
}

/**
 * Whether a spin mesh was built in the frame the full mesh is drawn in: its whole-model bounds
 * are the loaded model's box (X/Z centred, the plate at Y = 0) that the model's dimensions
 * describe. A mesh built some other way is not shown, so the swap never jumps.
 */
function spinMatchesModel(spin: SpinMesh, model: Model): boolean {
  const dims = [model.dimensionXMm, model.dimensionYMm, model.dimensionZMm];
  const expectedMin = [-(dims[0] ?? 0) / 2, 0, -(dims[2] ?? 0) / 2];
  return dims.every((dim, k) => {
    if (dim == null) return false;
    const tolerance = Math.max(dim * 0.005, 0.05);
    const min = spin.modelBounds.min[k];
    const max = spin.modelBounds.max[k];
    return Math.abs(min - expectedMin[k]) <= tolerance && Math.abs(max - min - dim) <= tolerance;
  });
}

function HullOverlays({
  convexHull,
  concaveHull,
  convexSansRaftHull,
  raftHeightMm,
}: HullOverlaysProps) {
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
      <HullPolygon coordinates={hullCoords} color={CONVEX_HULL_COLOR} />
      <HullLine coordinates={hullCoords} color={CONVEX_HULL_COLOR} />
      <HullPolygon
        coordinates={concaveCoords}
        color={CONCAVE_HULL_COLOR}
        yOffset={0.02}
        opacity={0.22}
      />
      <HullLine coordinates={concaveCoords} color={CONCAVE_HULL_COLOR} yOffset={0.02} />
      <HullPolygon
        coordinates={sansRaftCoords}
        color={SANS_RAFT_HULL_COLOR}
        yOffset={raftHeightMm}
        opacity={0.18}
      />
      <HullLine coordinates={sansRaftCoords} color={SANS_RAFT_HULL_COLOR} yOffset={raftHeightMm} />
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
    return geo;
  }, [data]);
  useEffect(() => () => bufferGeometry?.dispose(), [bufferGeometry]);
  const material = usePreviewMaterial(SUPPORT_COLOR, 0.5);

  if (!data || !bufferGeometry) return null;

  return <mesh geometry={bufferGeometry} material={material} visible={visible} />;
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
  const isSupportModel = supported === true;
  const {
    data: splitData,
    isPending: isSplitPending,
    isError: isSplitError,
  } = useSplitGeometry(modelId, isSupportModel);
  const shouldFetchFullGeometry = !isSupportModel || splitData === null;
  const {
    data: fullData,
    isPending: isFullGeometryPending,
    isError: isFullGeometryError,
  } = useGeometry(modelId, shouldFetchFullGeometry);
  const hasSupportMesh = isSupportModel && splitData?.supports != null;
  // Rebuilt from the original supports, so only asked for when there are some.
  const { data: resupported } = useResupportedGeometry(modelId, hasSupportMesh);
  const [chosenMode, setChosenMode] = useState<SupportMode | null>(null);
  const [showRepaired, setShowRepaired] = useState(false);
  const repaired = useRepairedGeometry(modelId, showRepaired);

  const modes: SupportMode[] = [
    ...(hasSupportMesh ? (['supported'] as const) : []),
    ...(resupported ? (['resupported'] as const) : []),
    'unsupported',
  ];
  const mode: SupportMode =
    chosenMode && modes.includes(chosenMode)
      ? chosenMode
      : hasSupportMesh
        ? 'supported'
        : 'unsupported';

  const color = MODEL_COLOR;
  const loadedBody = isSupportModel && splitData != null ? splitData.body : fullData;
  const repairedData = showRepaired ? repaired.data : undefined;
  const geometryData = repairedData?.geometry ?? loadedBody;
  // The reduced body stands in until the full mesh has loaded. Asked for only while the full
  // mesh is missing (so not at all when it is cached), alongside the full mesh's own requests.
  // A full mesh that failed to load leaves the viewer as it was without one.
  const fullFailed = isSupportModel
    ? isSplitError || (splitData === null && isFullGeometryError)
    : isFullGeometryError;
  const { data: spinData } = useSpinMesh(model?.spinUrl ?? null, loadedBody == null && !fullFailed);
  const preview =
    loadedBody == null &&
    !fullFailed &&
    spinData != null &&
    model != null &&
    spinMatchesModel(spinData, model)
      ? spinData
      : null;
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

  const isGeometryPending = isSupportModel
    ? isSplitPending || (splitData === null && isFullGeometryPending)
    : isFullGeometryPending;

  const errorFallback = (
    <div style={containerStyle}>
      <span style={messageStyle}>Couldn't load the 3D model.</span>
    </div>
  );

  if (isError) return errorFallback;

  if (
    isPending ||
    ((isGeometryPending || geometryData == null) && preview == null) ||
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
        <span style={messageStyle}>
          {isPending || isGeometryPending || (geometryData == null && preview == null)
            ? 'Loading model'
            : 'Model dimensions not available yet'}
        </span>
      </div>
    );
  }

  const repairing = showRepaired && repaired.isPending;
  // The reduced body is on screen: supports, the support modes and the repaired view wait for
  // the full mesh (the mode control appears with it, as the split decides which modes exist).
  const previewing = geometryData == null;

  return (
    <ViewerErrorBoundary fallback={errorFallback}>
      <div style={wrapperStyle}>
        <Canvas camera={{ fov: 45 }} gl={{ antialias: true }} style={containerStyle}>
          <SceneBg color={SCENE_BACKGROUND} />
          <CameraInit
            target={orbitTarget}
            halfExtents={halfExtents}
            direction={DEFAULT_VIEW_DIRECTION}
          />
          <Grid />
          {geometryData != null ? (
            <GeometryModel
              geometry={geometryData}
              changed={repairedData?.changed ?? null}
              color={color}
            />
          ) : (
            preview && <SpinPreviewModel spin={preview} color={color} />
          )}
          <HullOverlays
            convexHull={convexHull ?? null}
            concaveHull={concaveHull ?? null}
            convexSansRaftHull={convexSansRaftHull ?? null}
            raftHeightMm={model.raftHeightMm}
          />
          {hasSupportMesh && (
            <SupportGeometryMesh
              geometry={splitData?.supports ?? null}
              visible={mode === 'supported'}
            />
          )}
          {resupported && (
            <SupportGeometryMesh geometry={resupported} visible={mode === 'resupported'} />
          )}
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
        <div className={styles.controls}>
          {modes.length > 1 && (
            <SegmentedControl
              size="sm"
              aria-label="Supports"
              value={mode}
              onValueChange={(value) => setChosenMode(value as SupportMode)}
            >
              {modes.map((m) => (
                <SegmentedControlItem key={m} value={m}>
                  {MODE_LABELS[m]}
                </SegmentedControlItem>
              ))}
            </SegmentedControl>
          )}
          <Checkbox
            className={styles.repaired}
            disabled={previewing}
            checked={showRepaired}
            onCheckedChange={(checked) => setShowRepaired(checked === true)}
          >
            Repaired
          </Checkbox>
        </div>
        {showRepaired && repaired.isError && (
          <p className={`mm-type-small ${styles.error}`} role="alert">
            Couldn't repair the mesh.
          </p>
        )}
        {previewing && <Spinner className={styles.previewing} label="Loading model" />}
        {repairing && (
          <div className={styles.busy} role="status">
            <Spinner />
            <span className="mm-type-small">Repairing mesh</span>
          </div>
        )}
      </div>
    </ViewerErrorBoundary>
  );
}

const MODE_LABELS: Record<SupportMode, string> = {
  supported: 'Supported',
  resupported: 'Resupported',
  unsupported: 'Unsupported',
};

const wrapperStyle: React.CSSProperties = {
  position: 'relative',
  width: '100%',
  height: '100%',
};

const containerStyle: React.CSSProperties = {
  width: '100%',
  height: '100%',
  background: SCENE_BACKGROUND,
};

const messageStyle: React.CSSProperties = {
  font: '400 13px/18px var(--font-sans)',
  color: 'var(--muted-foreground)',
};
