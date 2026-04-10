import { createTheme } from '@mui/material/styles';

declare module '@mui/material/Chip' {
  interface ChipPropsVariantOverrides {
    'status-running': true;
    'status-queued': true;
    'badge-enabled': true;
    'badge-disabled': true;
  }
}

declare module '@mui/material/Typography' {
  interface TypographyPropsVariantOverrides {
    'section-label': true;
  }
}

declare module '@mui/material/Button' {
  interface ButtonPropsVariantOverrides {
    back: true;
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
        {
          props: { variant: 'badge-enabled' },
          style: {
            backgroundColor: 'rgba(99, 102, 241, 0.15)',
            color: '#818cf8',
            fontWeight: 600,
            fontSize: '0.65rem',
            height: 20,
            borderRadius: 4,
          },
        },
        {
          props: { variant: 'badge-disabled' },
          style: {
            backgroundColor: 'transparent',
            color: '#64748b',
            border: '1px solid #334155',
            fontWeight: 600,
            fontSize: '0.65rem',
            height: 20,
            borderRadius: 4,
          },
        },
      ],
    },
    MuiTypography: {
      variants: [
        {
          props: { variant: 'section-label' },
          style: {
            display: 'block',
            fontSize: '1.1rem',
            fontWeight: 600,
            color: '#94a3b8',
            textTransform: 'uppercase',
            letterSpacing: '0.08em',
            marginBottom: '0.75rem',
            marginLeft: '0.25rem',
          },
        },
      ],
    },
    MuiButton: {
      variants: [
        {
          props: { variant: 'back' },
          style: {
            position: 'fixed',
            top: 'calc(env(safe-area-inset-top, 0px) + 3.5rem + 0.75rem)',
            left: '1rem',
            background: 'rgba(15,23,42,0.7)',
            backdropFilter: 'blur(8px)',
            color: '#e2e8f0',
            border: '1px solid rgba(255,255,255,0.12)',
            borderRadius: '999px',
            padding: '0.5rem 1rem',
            fontSize: '0.9rem',
            fontWeight: 500,
            textTransform: 'none',
            zIndex: 10,
            minWidth: 0,
            '&:hover': { background: 'rgba(30,41,59,0.9)' },
            '&:active': { background: 'rgba(30,41,59,0.9)' },
          },
        },
      ],
    },
  },
});

export default theme;
