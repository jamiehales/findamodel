import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import { useNavigate } from 'react-router-dom'

interface PathBreadcrumbProps {
  /** Forward-slash separated path. Last segment is rendered as non-clickable. */
  path: string
}

export default function PathBreadcrumb({ path }: PathBreadcrumbProps) {
  const navigate = useNavigate()
  const segments = path === '' ? [] : path.split('/')

  return (
    <Box sx={{ display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: '4px' }}>
      <Typography
        component="span"
        onClick={() => navigate('/explore')}
        sx={{
          fontSize: '0.875rem',
          color: '#818cf8',
          cursor: 'pointer',
          '&:hover': { textDecoration: 'underline' },
        }}
      >
        findamodel
      </Typography>

      {segments.map((seg, i) => {
        const segPath = segments.slice(0, i + 1).join('/')
        const isLast = i === segments.length - 1
        return (
          <Box key={segPath} sx={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
            <Typography component="span" sx={{ color: '#475569', fontSize: '0.875rem' }}>
              /
            </Typography>
            <Typography
              component="span"
              onClick={() => !isLast && navigate(`/explore/${segPath}`)}
              sx={{
                fontSize: '0.875rem',
                color: isLast ? '#e2e8f0' : '#818cf8',
                cursor: isLast ? 'default' : 'pointer',
                fontWeight: isLast ? 600 : 400,
                '&:hover': !isLast ? { textDecoration: 'underline' } : {},
              }}
            >
              {seg}
            </Typography>
          </Box>
        )
      })}
    </Box>
  )
}
