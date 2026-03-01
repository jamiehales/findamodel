import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'

interface HullPreviewProps {
  convexHull: string | null
  concaveHull: string | null
  convexSansRaftHull: string | null
  label: string
}

type Coords = Array<[number, number]>

function parseHull(hullJson: string | null): Coords | null {
  if (!hullJson) return null
  try {
    return JSON.parse(hullJson)
  } catch {
    return null
  }
}

const SVG_SIZE = 200
const PADDING = 20
const VIEW_SIZE = SVG_SIZE - PADDING * 2

function computeBounds(coordSets: (Coords | null)[]) {
  const allX: number[] = []
  const allZ: number[] = []
  for (const coords of coordSets) {
    if (!coords) continue
    for (const [x, z] of coords) {
      allX.push(x)
      allZ.push(z)
    }
  }
  if (allX.length === 0) return null
  const minX = Math.min(...allX)
  const maxX = Math.max(...allX)
  const minZ = Math.min(...allZ)
  const maxZ = Math.max(...allZ)
  const scale = Math.max(maxX - minX, maxZ - minZ) || 1
  const centerX = (minX + maxX) / 2
  const centerZ = (minZ + maxZ) / 2
  return { offsetX: centerX - scale / 2, offsetZ: centerZ - scale / 2, scale }
}

function normalize(
  x: number,
  z: number,
  bounds: { offsetX: number; offsetZ: number; scale: number }
): [number, number] {
  return [
    ((x - bounds.offsetX) / bounds.scale) * VIEW_SIZE + PADDING,
    ((z - bounds.offsetZ) / bounds.scale) * VIEW_SIZE + PADDING,
  ]
}

function toPath(coords: Coords, bounds: { offsetX: number; offsetZ: number; scale: number }): string {
  const pts = coords.map(([x, z]) => normalize(x, z, bounds))
  return 'M ' + pts.map(([x, z]) => `${x},${z}`).join(' L ') + ' Z'
}

function EmptyPanel({ label }: { label: string }) {
  return (
    <Box
      sx={{
        flex: '1 1 45%',
        borderRadius: '12px',
        border: '1px solid rgba(255,255,255,0.07)',
        bgcolor: 'rgba(255,255,255,0.02)',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: '0.375rem',
        minHeight: 160,
        p: '1rem',
      }}
    >
      <Typography sx={{ fontSize: '0.75rem', fontWeight: 600, color: '#475569', textTransform: 'uppercase', letterSpacing: '0.05em' }}>
        {label}
      </Typography>
      <Typography sx={{ fontSize: '0.75rem', color: '#334155' }}>No data</Typography>
    </Box>
  )
}

interface PanelProps {
  label: string
  // Each layer: coords + stroke colour
  layers: Array<{ coords: Coords; color: string; label: string }>
}

function HullPanel({ label, layers }: PanelProps) {
  const bounds = computeBounds(layers.map(l => l.coords))
  if (!bounds) return <EmptyPanel label={label} />

  return (
    <Box
      sx={{
        flex: '1 1 45%',
        borderRadius: '12px',
        border: '1px solid rgba(255,255,255,0.07)',
        bgcolor: 'rgba(255,255,255,0.02)',
        p: '1rem',
        display: 'flex',
        flexDirection: 'column',
        gap: '0.5rem',
      }}
    >
      <Box sx={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <Typography sx={{ fontSize: '0.75rem', fontWeight: 600, color: '#94a3b8', textTransform: 'uppercase', letterSpacing: '0.05em' }}>
          {label}
        </Typography>
        {layers.length > 1 && (
          <Box sx={{ display: 'flex', gap: '0.625rem' }}>
            {layers.map(l => (
              <Box key={l.label} sx={{ display: 'flex', alignItems: 'center', gap: '0.3rem' }}>
                <Box sx={{ width: 8, height: 8, borderRadius: '50%', bgcolor: l.color, opacity: 0.9 }} />
                <Typography sx={{ fontSize: '0.65rem', color: '#64748b' }}>{l.label}</Typography>
              </Box>
            ))}
          </Box>
        )}
      </Box>
      <svg viewBox={`0 0 ${SVG_SIZE} ${SVG_SIZE}`} style={{ width: '100%', height: 'auto', aspectRatio: '1 / 1' }}>
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
                const [nx, nz] = normalize(x, z, bounds)
                return <circle key={vi} cx={nx} cy={nz} r="2" fill={l.color} opacity="0.8" />
              })}
          </g>
        ))}
      </svg>
    </Box>
  )
}

export default function HullPreview({ convexHull, concaveHull, convexSansRaftHull }: HullPreviewProps) {
  const convexCoords = parseHull(convexHull)
  const concaveCoords = parseHull(concaveHull)
  const sansRaftCoords = parseHull(convexSansRaftHull)

  return (
    <Box sx={{ display: 'flex', flexWrap: 'wrap', gap: '1rem', width: '100%' }}>
      {convexCoords && convexCoords.length >= 2 ? (
        <HullPanel label="Convex Hull" layers={[{ coords: convexCoords, color: '#818cf8', label: 'Convex' }]} />
      ) : (
        <EmptyPanel label="Convex Hull" />
      )}

      {(concaveCoords && concaveCoords.length >= 2) ? (
        <HullPanel label="Concave Hull" layers={[{ coords: concaveCoords, color: '#34d399', label: 'Concave' }]} />
      ) : (
        <EmptyPanel label="Concave Hull" />
      )}

      {/* Sans-raft panel: overlay full convex (muted) + sans-raft (highlighted) so the raft zone is visible */}
      {sansRaftCoords && sansRaftCoords.length >= 2 ? (
        <HullPanel
          label="Sans Raft"
          layers={[
            ...(convexCoords && convexCoords.length >= 2 ? [{ coords: convexCoords, color: '#818cf8', label: 'Full' }] : []),
            { coords: sansRaftCoords, color: '#f59e0b', label: 'Sans raft' },
          ]}
        />
      ) : (
        <EmptyPanel label="Sans Raft" />
      )}
    </Box>
  )
}
