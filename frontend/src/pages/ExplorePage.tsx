import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import Skeleton from '@mui/material/Skeleton'
import { useNavigate, useParams } from 'react-router-dom'
import { useExplorer } from '../lib/queries'
import FolderCard from '../components/FolderCard'
import ExplorerModelCard from '../components/ExplorerModelCard'

const gridSx = {
  display: 'grid',
  gridTemplateColumns: 'repeat(auto-fill, minmax(160px, 1fr))',
  gap: '0.875rem',
  alignItems: 'start',
}

function Breadcrumb({ path }: { path: string }) {
  const navigate = useNavigate()
  const segments = path === '' ? [] : path.split('/')

  return (
    <Box sx={{ display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: '4px', mb: '1.5rem' }}>
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

function ExplorePageInner({ path }: { path: string }) {
  const navigate = useNavigate()
  const { data, isPending, isError } = useExplorer(path)

  if (isPending) {
    return (
      <Box sx={gridSx}>
        {[1, 2, 3, 4, 5, 6].map(i => (
          <Skeleton
            key={i}
            variant="rectangular"
            sx={{ aspectRatio: '3 / 4', borderRadius: '14px', height: 'auto' }}
          />
        ))}
      </Box>
    )
  }

  if (isError) {
    return (
      <Typography sx={{ color: 'error.main', mt: 2 }}>
        Failed to load directory. Check that the path exists.
      </Typography>
    )
  }

  const isEmpty = data.folders.length === 0 && data.models.length === 0

  return (
    <>
      {isEmpty && (
        <Typography sx={{ color: 'text.disabled', mt: 2 }}>
          This folder is empty.
        </Typography>
      )}

      {data.folders.length > 0 && (
        <Box sx={{ mb: data.models.length > 0 ? '2rem' : 0 }}>
          <Typography
            sx={{
              fontSize: '1.1rem',
              fontWeight: 600,
              color: '#94a3b8',
              textTransform: 'uppercase',
              letterSpacing: '0.08em',
              mb: '0.75rem',
              ml: '0.25rem',
            }}
          >
            Folders
          </Typography>
          <Box sx={gridSx}>
            {data.folders.map(folder => (
              <FolderCard
                key={folder.path}
                folder={folder}
                onNavigate={() => navigate(`/explore/${folder.path}`)}
              />
            ))}
          </Box>
        </Box>
      )}

      {data.models.length > 0 && (
        <Box>
          <Typography
            sx={{
              fontSize: '1.1rem',
              fontWeight: 600,
              color: '#94a3b8',
              textTransform: 'uppercase',
              letterSpacing: '0.08em',
              mb: '0.75rem',
              ml: '0.25rem',
            }}
          >
            Models
          </Typography>
          <Box sx={gridSx}>
            {data.models.map(model => (
              <ExplorerModelCard
                key={model.relativePath}
                model={model}
                onClick={() => {
                  if (model.id) navigate(`/model/${encodeURIComponent(model.id)}`)
                }}
              />
            ))}
          </Box>
        </Box>
      )}
    </>
  )
}

export default function ExplorePage() {
  const navigate = useNavigate()
  const params = useParams()
  const path = params['*'] ?? ''

  return (
    <Box
      sx={{
        display: 'flex',
        flexDirection: 'column',
        minHeight: '100vh',
        px: { xs: 2, sm: 3 },
        pt: { xs: '3.5rem', sm: '5rem' },
        pb: '3rem',
        boxSizing: 'border-box',
        maxWidth: 1200,
        mx: 'auto',
        width: '100%',
      }}
    >
      {/* Header */}
      <Box sx={{ mb: '1rem' }}>
        <Typography
          component="button"
          onClick={() => navigate('/')}
          sx={{
            background: 'none',
            border: 'none',
            p: 0,
            cursor: 'pointer',
            fontSize: '0.875rem',
            color: '#475569',
            '&:hover': { color: '#818cf8' },
            mb: '0.5rem',
            display: 'block',
          }}
        >
          ← Home
        </Typography>

        <Typography
          component="h1"
          sx={{
            fontSize: '2rem',
            fontWeight: 700,
            letterSpacing: '-0.02em',
            lineHeight: 1,
            mb: '1.5rem',
          }}
        >
          find
          <Box component="span" sx={{ fontSize: '1.1rem', verticalAlign: 'baseline' }}>
            a
          </Box>
          model
          <Box component="span" sx={{ color: '#475569', fontSize: '1rem', fontWeight: 400, ml: '0.5rem' }}>
            / explore
          </Box>
        </Typography>

        <Breadcrumb path={path} />
      </Box>

      <ExplorePageInner path={path} />
    </Box>
  )
}
