import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'

interface HullPreviewProps {
  convexHull: string | null
  concaveHull: string | null
  label: string
}

export default function HullPreview({ convexHull, concaveHull }: HullPreviewProps) {
  const parseHull = (hullJson: string | null): Array<[number, number]> | null => {
    if (!hullJson) return null
    try {
      return JSON.parse(hullJson)
    } catch {
      return null
    }
  }

  const convexCoords = parseHull(convexHull)
  const concaveCoords = parseHull(concaveHull)

  const renderHullSvg = (coords: Array<[number, number]> | null, color: string, label: string) => {
    if (!coords || coords.length < 2) {
      return (
        <Box
          sx={{
            flex: 1,
            borderRadius: '12px',
            border: '1px solid rgba(255,255,255,0.07)',
            bgcolor: 'rgba(255,255,255,0.02)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            minHeight: 200,
          }}
        >
          <Typography sx={{ fontSize: '0.8rem', color: '#475569' }}>No hull data</Typography>
        </Box>
      )
    }

    // Find bounds
    const xs = coords.map(c => c[0])
    const zs = coords.map(c => c[1])
    const minX = Math.min(...xs)
    const maxX = Math.max(...xs)
    const minZ = Math.min(...zs)
    const maxZ = Math.max(...zs)
    const rangeX = maxX - minX || 1
    const rangeZ = maxZ - minZ || 1
    const scale = Math.max(rangeX, rangeZ)
    const centerX = (minX + maxX) / 2
    const centerZ = (minZ + maxZ) / 2
    const offsetX = centerX - scale / 2
    const offsetZ = centerZ - scale / 2

    // Normalize to SVG space (200x200 with padding)
    const svgSize = 200
    const padding = 20
    const viewSize = svgSize - padding * 2

    const normalize = (x: number, z: number): [number, number] => {
      const nx = ((x - offsetX) / scale) * viewSize + padding
      const nz = ((z - offsetZ) / scale) * viewSize + padding
      return [nx, nz]
    }

    const points = coords.map(([x, z]) => normalize(x, z))
    const pathData =
      'M ' +
      points.map(([x, z]) => `${x},${z}`).join(' L ') +
      ' Z'

    return (
      <Box
        sx={{
          flex: 1,
          borderRadius: '12px',
          border: `1px solid rgba(255,255,255,0.07)`,
          bgcolor: 'rgba(255,255,255,0.02)',
          p: '1rem',
          display: 'flex',
          flexDirection: 'column',
          gap: '0.5rem',
        }}
      >
        <Typography sx={{ fontSize: '0.75rem', fontWeight: 600, color: '#94a3b8', textTransform: 'uppercase', letterSpacing: '0.05em' }}>
          {label}
        </Typography>
        <svg
          viewBox={`0 0 ${svgSize} ${svgSize}`}
          style={{
            width: '100%',
            height: 'auto',
            aspectRatio: '1 / 1',
          }}
        >
          {/* Grid background */}
          <defs>
            <pattern id={`grid-${label}`} width="20" height="20" patternUnits="userSpaceOnUse">
              <path
                d="M 20 0 L 0 0 0 20"
                fill="none"
                stroke="#3a4559"
                strokeWidth="0.5"
              />
            </pattern>
          </defs>
          <rect width={svgSize} height={svgSize} fill={`url(#grid-${label})`} />

          {/* Hull polygon */}
          <path
            d={pathData}
            fill={color}
            fillOpacity="0.2"
            stroke={color}
            strokeWidth="1.5"
          />

          {/* Vertices */}
          {points.map(([x, z], i) => (
            <circle key={i} cx={x} cy={z} r="2" fill={color} opacity="0.8" />
          ))}
        </svg>
      </Box>
    )
  }

  return (
    <Box sx={{ display: 'flex', gap: '1rem', width: '100%' }}>
      {renderHullSvg(convexCoords, '#818cf8', 'Convex Hull')}
      {renderHullSvg(concaveCoords, '#34d399', 'Concave Hull')}
    </Box>
  )
}
