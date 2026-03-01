import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import { usePrintingList } from '../lib/printingList'

const btnSx = {
  pointerEvents: 'auto',
  width: 26, height: 26,
  borderRadius: '50%',
  border: '1px solid rgba(255,255,255,0.18)',
  background: 'rgba(15,23,42,0.7)',
  color: '#e2e8f0',
  fontSize: '1rem',
  lineHeight: 1,
  cursor: 'pointer',
  display: 'flex', alignItems: 'center', justifyContent: 'center',
  p: 0,
  '&:hover': { background: 'rgba(30,41,59,0.9)' },
  '&:active': { transform: 'scale(0.92)' },
} as const

interface Props {
  modelId: string
}

export default function PrintingListControls({ modelId }: Props) {
  const { items, setQuantity } = usePrintingList()
  const quantity = items[modelId]

  return (
    <Box
      sx={{
        position: 'absolute',
        top: 0,
        left: 0,
        right: 0,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        px: '0.4rem',
        py: '0.35rem',
        zIndex: 2,
        background: 'linear-gradient(to bottom, rgba(15,23,42,0.75) 0%, transparent 100%)',
        pointerEvents: 'none',
      }}
    >
      <Box
        component="button"
        onClick={e => { e.preventDefault(); e.stopPropagation(); setQuantity(modelId, (quantity ?? 0) - 1) }}
        sx={btnSx}
      >
        −
      </Box>

      <Typography
        sx={{
          color: '#e2e8f0',
          fontSize: '0.8rem',
          fontWeight: 700,
          letterSpacing: '0.02em',
          textShadow: '0 1px 4px rgba(0,0,0,0.6)',
          pointerEvents: 'none',
          visibility: quantity !== undefined ? 'visible' : 'hidden',
        }}
      >
        ×{quantity ?? 0}
      </Typography>

      <Box
        component="button"
        onClick={e => { e.preventDefault(); e.stopPropagation(); setQuantity(modelId, (quantity ?? 0) + 1) }}
        sx={btnSx}
      >
        +
      </Box>
    </Box>
  )
}
