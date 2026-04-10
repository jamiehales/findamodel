import Box from '@mui/material/Box';
import Typography from '@mui/material/Typography';
import styles from './HullPreview.module.css';

interface HullPreviewProps {
  convexHull: string | null;
  concaveHull: string | null;
  convexSansRaftHull: string | null;
  label: string;
}

type Coords = Array<[number, number]>;

function parseHull(hullJson: string | null): Coords | null {
  if (!hullJson) return null;
  try {
    return JSON.parse(hullJson);
  } catch {
    return null;
  }
}

const SVG_SIZE = 200;
const PADDING = 20;
const VIEW_SIZE = SVG_SIZE - PADDING * 2;

function computeBounds(coordSets: (Coords | null)[]) {
  const allX: number[] = [];
  const allZ: number[] = [];
  for (const coords of coordSets) {
    if (!coords) continue;
    for (const [x, z] of coords) {
      allX.push(x);
      allZ.push(z);
    }
  }
  if (allX.length === 0) return null;
  const minX = Math.min(...allX);
  const maxX = Math.max(...allX);
  const minZ = Math.min(...allZ);
  const maxZ = Math.max(...allZ);
  const scale = Math.max(maxX - minX, maxZ - minZ) || 1;
  const centerX = (minX + maxX) / 2;
  const centerZ = (minZ + maxZ) / 2;
  return { offsetX: centerX - scale / 2, offsetZ: centerZ - scale / 2, scale };
}

function normalize(
  x: number,
  z: number,
  bounds: { offsetX: number; offsetZ: number; scale: number },
): [number, number] {
  return [
    ((x - bounds.offsetX) / bounds.scale) * VIEW_SIZE + PADDING,
    ((z - bounds.offsetZ) / bounds.scale) * VIEW_SIZE + PADDING,
  ];
}

function toPath(
  coords: Coords,
  bounds: { offsetX: number; offsetZ: number; scale: number },
): string {
  const pts = coords.map(([x, z]) => normalize(x, z, bounds));
  return 'M ' + pts.map(([x, z]) => `${x},${z}`).join(' L ') + ' Z';
}

function EmptyPanel({ label }: { label: string }) {
  return (
    <Box className={styles.emptyPanel}>
      <Typography className={styles.emptyLabel}>{label}</Typography>
      <Typography className={styles.emptyNoData}>No data</Typography>
    </Box>
  );
}

interface PanelProps {
  label: string;
  // Each layer: coords + stroke colour
  layers: Array<{ coords: Coords; color: string; label: string }>;
}

function HullPanel({ label, layers }: PanelProps) {
  const bounds = computeBounds(layers.map((l) => l.coords));
  if (!bounds) return <EmptyPanel label={label} />;

  return (
    <Box className={styles.panel}>
      <Box className={styles.panelHeader}>
        <Typography className={styles.panelLabel}>{label}</Typography>
        {layers.length > 1 && (
          <Box className={styles.legend}>
            {layers.map((l) => (
              <Box key={l.label} className={styles.legendItem}>
                <Box className={styles.legendDot} style={{ backgroundColor: l.color }} />
                <Typography className={styles.legendLabel}>{l.label}</Typography>
              </Box>
            ))}
          </Box>
        )}
      </Box>
      <svg
        viewBox={`0 0 ${SVG_SIZE} ${SVG_SIZE}`}
        style={{ width: '100%', height: 'auto', aspectRatio: '1 / 1' }}
      >
        <defs>
          <pattern id={`grid-${label}`} width="20" height="20" patternUnits="userSpaceOnUse">
            <path d="M 20 0 L 0 0 0 20" fill="none" stroke="#3a4559" strokeWidth="0.5" />
          </pattern>
        </defs>
        <rect width={SVG_SIZE} height={SVG_SIZE} fill={`url(#grid-${label})`} />

        {layers.map((l, i) => (
          <g key={i}>
            <path
              d={toPath(l.coords, bounds)}
              fill={l.color}
              fillOpacity={i === 0 ? 0.08 : 0.18}
              stroke={l.color}
              strokeWidth={i === 0 ? 1 : 1.5}
              strokeOpacity={i === 0 ? 0.5 : 1}
            />
            {i === layers.length - 1 &&
              l.coords.map(([x, z], vi) => {
                const [nx, nz] = normalize(x, z, bounds);
                return <circle key={vi} cx={nx} cy={nz} r="2" fill={l.color} opacity="0.8" />;
              })}
          </g>
        ))}
      </svg>
    </Box>
  );
}

export default function HullPreview({
  convexHull,
  concaveHull,
  convexSansRaftHull,
}: HullPreviewProps) {
  const convexCoords = parseHull(convexHull);
  const concaveCoords = parseHull(concaveHull);
  const sansRaftCoords = parseHull(convexSansRaftHull);

  const convexLayers = [
    ...(convexCoords && convexCoords.length >= 2
      ? [{ coords: convexCoords, color: '#818cf8', label: 'Full' }]
      : []),
    ...(sansRaftCoords && sansRaftCoords.length >= 2
      ? [{ coords: sansRaftCoords, color: '#f59e0b', label: 'Sans raft' }]
      : []),
  ];

  return (
    <Box className={styles.container}>
      {convexLayers.length > 0 ? (
        <HullPanel label="Convex" layers={convexLayers} />
      ) : (
        <EmptyPanel label="Convex" />
      )}

      {concaveCoords && concaveCoords.length >= 2 ? (
        <HullPanel
          label="Concave Hull"
          layers={[{ coords: concaveCoords, color: '#34d399', label: 'Concave' }]}
        />
      ) : (
        <EmptyPanel label="Concave Hull" />
      )}
    </Box>
  );
}
