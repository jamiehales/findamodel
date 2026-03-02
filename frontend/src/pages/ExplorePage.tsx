import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import Skeleton from '@mui/material/Skeleton'
import { useNavigate, useParams } from 'react-router-dom'
import { useExplorer } from '../lib/queries'
import FolderCard from '../components/FolderCard'
import ExplorerModelCard from '../components/ExplorerModelCard'
import PathBreadcrumb from '../components/PathBreadcrumb'
import styles from './ExplorePage.module.css'

function ExplorePageInner({ path }: { path: string }) {
  const { data, isPending, isError } = useExplorer(path)

  if (isPending) {
    return (
      <Box className={styles.grid}>
        {[1, 2, 3, 4, 5, 6].map(i => (
          <Skeleton
            key={i}
            variant="rectangular"
            className={styles.skeleton}
          />
        ))}
      </Box>
    )
  }

  if (isError) {
    return (
      <Typography color="error.main" style={{ marginTop: 16 }}>
        Failed to load directory. Check that the path exists.
      </Typography>
    )
  }

  const isEmpty = data.folders.length === 0 && data.models.length === 0

  return (
    <>
      {isEmpty && (
        <Typography color="text.disabled" style={{ marginTop: 16 }}>
          This folder is empty.
        </Typography>
      )}

      {data.folders.length > 0 && (
        <Box className={data.models.length > 0 ? styles.sectionWithMargin : undefined}>
          <Typography variant="section-label">
            Folders
          </Typography>
          <Box className={styles.grid}>
            {data.folders.map(folder => (
              <FolderCard
                key={folder.path}
                folder={folder}
                href={`/explore/${folder.path}`}
              />
            ))}
          </Box>
        </Box>
      )}

      {data.models.length > 0 && (
        <Box>
          <Typography variant="section-label">
            Models
          </Typography>
          <Box className={styles.grid}>
            {data.models.map(model => (
              <ExplorerModelCard
                key={model.relativePath}
                model={model}
                href={model.id ? `/model/${encodeURIComponent(model.id)}` : undefined}
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
    <Box className={styles.page}>
      {/* Header */}
      <Box className={styles.header}>
        <Typography
          component="button"
          onClick={() => navigate('/')}
          className={styles.homeLink}
        >
          ← Home
        </Typography>

        <Typography component="h1" className={styles.logoTitle}>
          find
          <Box component="span" className={styles.logoA}>
            a
          </Box>
          model
          <Box component="span" className={styles.logoSuffix}>
            / explore
          </Box>
        </Typography>

        <Box className={styles.breadcrumbWrapper}>
          <PathBreadcrumb path={path} />
        </Box>
      </Box>

      <ExplorePageInner path={path} />
    </Box>
  )
}
