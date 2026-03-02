import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import { useNavigate } from 'react-router-dom'
import styles from './PathBreadcrumb.module.css'

interface PathBreadcrumbProps {
  /** Forward-slash separated path. Last segment is rendered as non-clickable. */
  path: string
}

export default function PathBreadcrumb({ path }: PathBreadcrumbProps) {
  const navigate = useNavigate()
  const segments = path === '' ? [] : path.split('/')

  return (
    <Box className={styles.breadcrumb}>
      <Typography
        component="span"
        onClick={() => navigate('/explore')}
        className={styles.rootLink}
      >
        findamodel
      </Typography>

      {segments.map((seg, i) => {
        const segPath = segments.slice(0, i + 1).join('/')
        const isLast = i === segments.length - 1
        return (
          <Box key={segPath} className={styles.segment}>
            <Typography component="span" className={styles.separator}>
              /
            </Typography>
            <Typography
              component="span"
              onClick={() => !isLast && navigate(`/explore/${segPath}`)}
              className={isLast ? styles.segLinkLast : styles.segLink}
            >
              {seg}
            </Typography>
          </Box>
        )
      })}
    </Box>
  )
}
