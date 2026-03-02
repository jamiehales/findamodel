import { Link as RouterLink } from 'react-router-dom'
import Box from '@mui/material/Box'
import styles from './AppCard.module.css'

interface AppCardProps {
  href?: string
  className?: string
  children: React.ReactNode
}

export default function AppCard({ href, className, children }: AppCardProps) {
  const combinedClass = `${styles.base}${className ? ` ${className}` : ''}`

  if (!href) {
    return (
      <Box className={combinedClass}>
        {children}
      </Box>
    )
  }

  return (
    <Box
      component={RouterLink as React.ElementType}
      {...{ to: href }}
      className={combinedClass}
    >
      {children}
    </Box>
  )
}
