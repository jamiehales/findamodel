import { createTheme } from '@mui/material/styles'

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
    },
    text: {
      primary: '#e2e8f0',
      secondary: '#64748b',
    },
  },
  typography: {
    fontFamily: 'system-ui, -apple-system, sans-serif',
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
  },
})

export default theme
