import { Link as RouterLink } from 'react-router-dom'
import Box from '@mui/material/Box'
import type { SxProps, Theme } from '@mui/material/styles'

interface AppCardProps {
  href?: string
  sx?: SxProps<Theme>
  className?: string
  children: React.ReactNode
}

const baseSx: SxProps<Theme> = {
  textDecoration: 'none',
  color: 'inherit',
  '&:hover': { transform: 'scale(1.03)', boxShadow: '0 8px 24px rgba(0,0,0,0.4)' },
  '&:active': { transform: 'scale(0.97)' },
}

export default function AppCard({ href, sx, className, children }: AppCardProps) {
  const sxArr = [baseSx, ...(Array.isArray(sx) ? sx : sx ? [sx] : [])]

  if (!href) {
    return (
      <Box sx={sxArr} className={className}>
        {children}
      </Box>
    )
  }

  return (
    <Box
      component={RouterLink as React.ElementType}
      {...{ to: href }}
      sx={sxArr}
      className={className}
    >
      {children}
    </Box>
  )
}
