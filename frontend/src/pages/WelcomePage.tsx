import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import Typography from '@mui/material/Typography'
import { useNavigate } from 'react-router-dom'
import ModelGrid from '../components/ModelGrid'
import { useActivePrintingList } from '../lib/queries'

function WelcomePage() {
  const navigate = useNavigate()
  const { data: activeList } = useActivePrintingList()
  const totalCount = activeList?.items.reduce((a, i) => a + i.quantity, 0) ?? 0

  return (
    <Box
      sx={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        minHeight: '100vh',
        pb: '3rem',
        gap: '2rem',
      }}
    >
      <Box
        sx={{
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          pt: { xs: '3.5rem', sm: '5rem' },
          px: 2,
          textAlign: 'center',
          gap: '0.75rem',
        }}
      >
        <Typography
          component="h1"
          sx={{
            fontSize: '3rem',
            fontWeight: 700,
            letterSpacing: '-0.03em',
            mb: '0.5rem',
            lineHeight: 1,
          }}
        >
          find
          <Box component="span" sx={{ fontSize: '1.6rem', verticalAlign: 'baseline' }}>
            a
          </Box>
          model
        </Typography>
        <Typography sx={{ fontSize: '1.1rem', color: 'text.secondary' }}>
          Find your next mini
        </Typography>
        <Box sx={{ display: 'flex', gap: '0.75rem', flexWrap: 'wrap', justifyContent: 'center' }}>
          <Button
            onClick={() => navigate('/explore')}
            variant="outlined"
            sx={{
              borderRadius: '999px',
              px: '1.25rem',
              py: '0.4rem',
              fontSize: '0.875rem',
              fontWeight: 500,
              textTransform: 'none',
              borderColor: 'rgba(99,102,241,0.4)',
              color: '#818cf8',
              '&:hover': { borderColor: '#818cf8', background: 'rgba(99,102,241,0.08)' },
            }}
          >
            Explore
          </Button>
          {totalCount > 0 && (
            <Button
              onClick={() => navigate('/printing-list/active')}
              variant="outlined"
              sx={{
                borderRadius: '999px',
                px: '1.25rem',
                py: '0.4rem',
                fontSize: '0.875rem',
                fontWeight: 500,
                textTransform: 'none',
                borderColor: 'rgba(99,102,241,0.5)',
                color: '#818cf8',
                '&:hover': { borderColor: '#818cf8', background: 'rgba(99,102,241,0.08)' },
              }}
            >
              View printing list ({totalCount})
            </Button>
          )}
          <Button
            onClick={() => navigate('/printing-lists')}
            variant="outlined"
            sx={{
              borderRadius: '999px',
              px: '1.25rem',
              py: '0.4rem',
              fontSize: '0.875rem',
              fontWeight: 500,
              textTransform: 'none',
              borderColor: 'rgba(255,255,255,0.15)',
              color: '#94a3b8',
              '&:hover': { borderColor: 'rgba(255,255,255,0.3)', background: 'rgba(255,255,255,0.05)', color: '#e2e8f0' },
            }}
          >
            Manage printing lists
          </Button>
        </Box>
      </Box>

      <ModelGrid />
    </Box>
  )
}

export default WelcomePage
