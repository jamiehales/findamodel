import { createTheme } from '@mui/material/styles'

declare module '@mui/material/Chip' {
  interface ChipPropsVariantOverrides {
    'status-running': true
    'status-queued': true
  }
}

const theme = createTheme({
  palette: {
    mode: 'dark',
    background: {
      default: '#0f172a',
      paper: '#1e293b',
    },
    primary: {
      main: '#6366f1',
      dark: '#4f46e5',
      light: '#818cf8',
    },
    success: {
      main: '#34d399',
    },
    warning: {
      main: '#fbbf24',
    },
    text: {
      primary: '#e2e8f0',
      secondary: '#64748b',
    },
  },
  typography: {
    fontFamily: 'system-ui, -apple-system, sans-serif',
    overline: {
      fontWeight: 600,
    },
  },
  shape: {
    borderRadius: 14,
  },
  components: {
    MuiCssBaseline: {
      styleOverrides: {
        body: {
          backgroundColor: '#0f172a',
        },
      },
    },
    MuiChip: {
      variants: [
        {
          props: { variant: 'status-running' },
          style: {
            backgroundColor: 'rgba(52, 211, 153, 0.15)',
            color: '#34d399',
            fontWeight: 600,
            fontSize: '0.65rem',
            height: 20,
          },
        },
        {
          props: { variant: 'status-queued' },
          style: {
            backgroundColor: 'rgba(251, 191, 36, 0.15)',
            color: '#fbbf24',
            fontWeight: 600,
            fontSize: '0.65rem',
            height: 20,
          },
        },
      ],
    },
  },
})

export default theme
