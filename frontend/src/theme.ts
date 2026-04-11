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
    'page-title': true;
  }
}

declare module '@mui/material/Button' {
  interface ButtonPropsVariantOverrides {
    back: true;
    warning: true;
    primary: true;
    activate: true;
  }
}

export type ThemeName = 'default' | 'nord';

const sharedTypography = {
  fontFamily: '"Inter", system-ui, -apple-system, sans-serif',
  overline: {
    fontWeight: 600,
  },
} as const;

const sharedShape = {
  borderRadius: 14,
} as const;

const sharedComponents = {
  MuiToolbar: {
    styleOverrides: {
      root: {
        minHeight: 56,
        paddingLeft: '1rem',
        paddingRight: '1rem',
        ['@media (min-width: 600px)']: {
          paddingLeft: '1.5rem',
          paddingRight: '1.5rem',
        },
      },
    },
  },
  MuiChip: {
    variants: [
      {
        props: { variant: 'status-running' as const },
        style: {
          backgroundColor: 'rgba(52, 211, 153, 0.15)',
          color: '#34d399',
          fontWeight: 600,
          fontSize: '0.65rem',
          height: 20,
        },
      },
      {
        props: { variant: 'status-queued' as const },
        style: {
          backgroundColor: 'rgba(251, 191, 36, 0.15)',
          color: '#fbbf24',
          fontWeight: 600,
          fontSize: '0.65rem',
          height: 20,
        },
      },
      {
        props: { variant: 'badge-enabled' as const },
        style: {
          backgroundColor: '#312e81',
          color: '#c7d2fe',
          fontWeight: 600,
          fontSize: '0.65rem',
          height: 20,
          borderRadius: 4,
        },
      },
      {
        props: { variant: 'badge-disabled' as const },
        style: {
          backgroundColor: '#1e2535',
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
        props: { variant: 'section-label' as const },
        style: {
          display: 'block',
          fontSize: '1.1rem',
          fontWeight: 600,
          color: '#94a3b8',
          textTransform: 'uppercase' as const,
          letterSpacing: '0.08em',
          marginBottom: '0.75rem',
          marginLeft: '0.25rem',
        },
      },
      {
        props: { variant: 'page-title' as const },
        style: {
          fontSize: '2rem',
          fontWeight: 700,
          letterSpacing: '-0.02em',
          color: '#f1f5f9',
          lineHeight: 1.2,
          ['@media (min-width: 600px)']: {
            fontSize: '2.5rem',
          },
        },
      },
      {
        props: { variant: 'h5' as const },
        style: {
          fontSize: '1.35rem',
          fontWeight: 600,
          letterSpacing: '-0.01em',
          color: '#f1f5f9',
          lineHeight: 1.3,
        },
      },
      {
        props: { variant: 'h6' as const },
        style: {
          fontSize: '1.1rem',
          fontWeight: 600,
          letterSpacing: '-0.005em',
          color: '#cbd5e1',
          lineHeight: 1.4,
        },
      },
    ],
  },
  MuiButton: {
    defaultProps: {
      disableElevation: true,
    },
    styleOverrides: {
      root: {
        borderRadius: '999px',
        padding: '0.4rem 1rem',
        fontSize: '0.85rem',
        fontWeight: 500,
        textTransform: 'none' as const,
        minWidth: 0,
      },
      contained: {
        color: '#94a3b8',
        border: '1px solid rgba(255, 255, 255, 0.1)',
        backgroundColor: 'transparent',
        boxShadow: 'none',
        '&:hover': {
          backgroundColor: 'rgba(255, 255, 255, 0.06)',
          color: '#e2e8f0',
          boxShadow: 'none',
        },
      },
    },
    variants: [
      {
        props: { variant: 'back' as const },
        style: {
          position: 'fixed' as const,
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
          textTransform: 'none' as const,
          zIndex: 10,
          minWidth: 0,
          '&:hover': { background: 'rgba(30,41,59,0.9)' },
          '&:active': { background: 'rgba(30,41,59,0.9)' },
        },
      },
      {
        props: { variant: 'warning' as const },
        style: {
          borderRadius: '999px',
          padding: '0.4rem 1rem',
          fontSize: '0.85rem',
          fontWeight: 500,
          textTransform: 'none' as const,
          minWidth: 0,
          color: '#f87171',
          border: '1px solid rgba(248, 113, 113, 0.25)',
          backgroundColor: 'transparent',
          '&:hover': {
            backgroundColor: 'rgba(248, 113, 113, 0.1)',
          },
        },
      },
      {
        props: { variant: 'primary' as const },
        style: {
          borderRadius: '999px',
          padding: '0.4rem 1rem',
          fontSize: '0.85rem',
          fontWeight: 600,
          textTransform: 'none' as const,
          minWidth: 0,
          color: '#ffffff',
          border: '1px solid #4f46e5',
          backgroundColor: '#6366f1',
          boxShadow: 'none',
          '&:hover': {
            backgroundColor: '#4f46e5',
            boxShadow: 'none',
          },
          '&:disabled': {
            backgroundColor: 'rgba(99, 102, 241, 0.2)',
            color: 'rgba(255,255,255,0.3)',
            border: '1px solid rgba(99, 102, 241, 0.15)',
          },
        },
      },
      {
        props: { variant: 'activate' as const },
        style: {
          background: 'rgba(255, 255, 255, 0.06)',
          backdropFilter: 'blur(8px)',
          color: '#94a3b8',
          border: '1px solid rgba(255, 255, 255, 0.1)',
          borderRadius: 999,
          padding: '0.5rem 1.25rem',
          fontSize: '0.9rem',
          fontWeight: 500,
          textTransform: 'none' as const,
          minWidth: 0,
          '&:hover': {
            background: 'rgba(255, 255, 255, 0.1)',
            color: '#e2e8f0',
          },
        },
      },
    ],
  },
  MuiDialogTitle: {
    styleOverrides: {
      root: {
        padding: '0.875rem 1rem',
        color: '#f8fafc',
      },
    },
  },
  MuiDialogContent: {
    styleOverrides: {
      root: {
        padding: '0.875rem 1rem',
        color: '#e2e8f0',
      },
    },
  },
  MuiDialogActions: {
    styleOverrides: {
      root: {
        padding: '0.875rem 1rem',
        gap: '4px',
        '& .MuiButton-root': {
          borderRadius: '999px',
          textTransform: 'none',
        },
        '& .MuiButton-outlined': {
          borderColor: 'rgba(148, 163, 184, 0.65)',
          color: '#e2e8f0',
        },
        '& .MuiButton-outlined:hover': {
          borderColor: 'rgba(226, 232, 240, 0.9)',
          backgroundColor: 'rgba(148, 163, 184, 0.12)',
        },
      },
    },
  },
};

function buildDefaultTheme() {
  return createTheme({
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
    typography: sharedTypography,
    shape: sharedShape,
    components: {
      ...sharedComponents,
      MuiCssBaseline: {
        styleOverrides: {
          body: {
            backgroundColor: '#0f172a',
          },
        },
      },
      MuiAppBar: {
        styleOverrides: {
          root: {
            background: 'rgba(15, 23, 42, 0.9)',
            backdropFilter: 'blur(12px)',
            WebkitBackdropFilter: 'blur(12px)',
            borderBottom: '1px solid rgba(255, 255, 255, 0.07)',
            boxShadow: 'none',
          },
        },
      },
      MuiDialog: {
        styleOverrides: {
          paper: {
            backgroundColor: '#070b14',
            border: '1px solid rgba(99, 102, 241, 0.25)',
          },
        },
      },
    },
  });
}

function buildNordTheme() {
  return createTheme({
    palette: {
      mode: 'dark',
      background: {
        default: '#2e3440',
        paper: '#3b4252',
      },
      primary: {
        main: '#88c0d0',
        dark: '#5e81ac',
        light: '#81a1c1',
      },
      success: { main: '#a3be8c' },
      warning: { main: '#ebcb8b' },
      error: { main: '#bf616a' },
      text: {
        primary: '#eceff4',
        secondary: '#d8dee9',
      },
    },
    typography: sharedTypography,
    shape: sharedShape,
    components: {
      ...sharedComponents,
      MuiCssBaseline: {
        styleOverrides: {
          body: {
            backgroundColor: '#2e3440',
          },
        },
      },
      MuiAppBar: {
        styleOverrides: {
          root: {
            background: 'rgba(46, 52, 64, 0.9)',
            backdropFilter: 'blur(12px)',
            WebkitBackdropFilter: 'blur(12px)',
            borderBottom: '1px solid rgba(255, 255, 255, 0.07)',
            boxShadow: 'none',
          },
        },
      },
      MuiDialog: {
        styleOverrides: {
          paper: {
            backgroundColor: '#434c5e',
            border: '1px solid rgba(136, 192, 208, 0.25)',
          },
        },
      },
    },
  });
}

export function buildTheme(name: ThemeName) {
  return name === 'nord' ? buildNordTheme() : buildDefaultTheme();
}

// Backward-compatible default export
export default buildDefaultTheme();

/** Semantic colour tokens shared across components — derived from theme palette */
export function buildAppColors(_themeName: ThemeName) {
  return {
    fileType: {
      stl: { bg: 'rgba(99,102,241,0.2)', color: '#818cf8' },
      obj: { bg: 'rgba(16,185,129,0.2)', color: '#34d399' },
      ctb: { bg: 'rgba(217, 119, 6, 0.2)', color: '#fdba74' },
      lys: { bg: 'rgba(190, 24, 93, 0.2)', color: '#f9a8d4' },
      lyt: { bg: 'rgba(225, 29, 72, 0.2)', color: '#fda4af' },
      png: { bg: 'rgba(59, 130, 246, 0.2)', color: '#93c5fd' },
      jpg: { bg: 'rgba(56, 189, 248, 0.2)', color: '#7dd3fc' },
      jpeg: { bg: 'rgba(56, 189, 248, 0.2)', color: '#7dd3fc' },
      gif: { bg: 'rgba(16, 185, 129, 0.2)', color: '#6ee7b7' },
      webp: { bg: 'rgba(20, 184, 166, 0.2)', color: '#99f6e4' },
      txt: { bg: 'rgba(148, 163, 184, 0.2)', color: '#cbd5e1' },
      md: { bg: 'rgba(129, 140, 248, 0.2)', color: '#c7d2fe' },
      // add new file types here
    } as Record<string, { bg: string; color: string }>,
    metaBadge: {
      rule: '#fbbf24',
      value: '#a5b4fc',
    },
  } as const;
}

// Backward-compatible named export
export const appColors = buildAppColors('default');
