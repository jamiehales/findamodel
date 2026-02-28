import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import ModelGrid from '../components/ModelGrid'

function WelcomePage() {
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
      </Box>

      <ModelGrid />
    </Box>
  )
}

export default WelcomePage
