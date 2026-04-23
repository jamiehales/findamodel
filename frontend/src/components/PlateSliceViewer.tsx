import { useEffect, useMemo, useState } from 'react';
import { Canvas } from '@react-three/fiber';
import { OrbitControls } from '@react-three/drei';
import * as THREE from 'three';
import { useTheme } from '@mui/material';
import { getPlateSlicePreviewLayerUrl, type PlateSlicePreviewSession } from '../lib/api';

const textureCache = new Map<string, THREE.Texture>();

interface SlicePlaneProps {
  textureUrl: string;
  bedWidthMm: number;
  bedDepthMm: number;
  sliceHeightMm: number;
}

function SlicePlane({ textureUrl, bedWidthMm, bedDepthMm, sliceHeightMm }: SlicePlaneProps) {
  const [texture, setTexture] = useState<THREE.Texture | null>(null);

  useEffect(() => {
    const cached = textureCache.get(textureUrl);
    if (cached) {
      setTexture(cached);
      return;
    }

    let cancelled = false;
    const loader = new THREE.TextureLoader();
    loader.load(textureUrl, (loadedTexture) => {
      if (cancelled) return;

      loadedTexture.minFilter = THREE.NearestFilter;
      loadedTexture.magFilter = THREE.NearestFilter;
      loadedTexture.wrapS = THREE.ClampToEdgeWrapping;
      loadedTexture.wrapT = THREE.ClampToEdgeWrapping;
      loadedTexture.flipY = false;
      loadedTexture.needsUpdate = true;

      textureCache.set(textureUrl, loadedTexture);
      setTexture(loadedTexture);
    });

    return () => {
      cancelled = true;
    };
  }, [textureUrl]);

  if (!texture) return null;

  return (
    <mesh position={[0, sliceHeightMm, 0]} rotation={[-Math.PI / 2, 0, 0]}>
      <planeGeometry args={[bedWidthMm, bedDepthMm]} />
      <meshBasicMaterial
        map={texture}
        transparent
        opacity={0.82}
        depthWrite={false}
        side={THREE.DoubleSide}
      />
    </mesh>
  );
}

interface PlateSliceViewerProps {
  session: PlateSlicePreviewSession;
  selectedLayerIndex: number;
}

export default function PlateSliceViewer({ session, selectedLayerIndex }: PlateSliceViewerProps) {
  const theme = useTheme();
  const clampedLayer = Math.min(
    Math.max(selectedLayerIndex, 0),
    Math.max(session.layerCount - 1, 0),
  );
  const sliceHeightMm = clampedLayer * session.layerHeightMm + session.layerHeightMm * 0.5;
  const textureUrl = useMemo(
    () => getPlateSlicePreviewLayerUrl(session.previewId, clampedLayer),
    [clampedLayer, session.previewId],
  );

  const target: [number, number, number] = [0, 0, 0];
  const maxBedExtent = Math.max(session.bedWidthMm, session.bedDepthMm);
  const cameraHeight = Math.max(40, maxBedExtent * 0.7);
  const cameraDistance = Math.max(90, maxBedExtent * 0.8);

  return (
    <div style={containerStyle}>
      <Canvas camera={{ fov: 44, position: [cameraDistance, cameraHeight, -cameraDistance] }}>
        <color attach="background" args={[theme.palette.background.default]} />
        <ambientLight intensity={0.5} />
        <directionalLight position={[80, 140, 40]} intensity={1.1} />
        <gridHelper
          args={[Math.max(session.bedWidthMm, session.bedDepthMm), 16, '#2e3b55', '#1e293b']}
          position={[0, 0, 0]}
        />
        <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, -0.01, 0]}>
          <planeGeometry args={[session.bedWidthMm, session.bedDepthMm]} />
          <meshStandardMaterial color="#0f172a" roughness={0.95} metalness={0.08} />
        </mesh>
        <SlicePlane
          textureUrl={textureUrl}
          bedWidthMm={session.bedWidthMm}
          bedDepthMm={session.bedDepthMm}
          sliceHeightMm={sliceHeightMm}
        />
        <OrbitControls
          target={target}
          enableDamping
          dampingFactor={0.08}
          minDistance={20}
          maxDistance={Math.max(500, maxBedExtent * 4)}
        />
      </Canvas>
    </div>
  );
}

const containerStyle: React.CSSProperties = {
  width: '100%',
  minHeight: '420px',
  height: '60vh',
  borderRadius: '16px',
  overflow: 'hidden',
  border: '1px solid rgba(148, 163, 184, 0.2)',
};
