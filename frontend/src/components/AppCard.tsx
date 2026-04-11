import { Link as RouterLink } from 'react-router-dom';
import Box from '@mui/material/Box';
import styles from './AppCard.module.css';

interface AppCardProps {
  href?: string;
  className?: string;
  children: React.ReactNode;
  onMouseEnter?: () => void;
  onMouseLeave?: () => void;
  onClick?: () => void;
  interactive?: boolean;
}

export default function AppCard({
  href,
  className,
  children,
  onMouseEnter,
  onMouseLeave,
  onClick,
  interactive = false,
}: AppCardProps) {
  const combinedClass = `${styles.base}${interactive ? ` ${styles.interactiveSurface}` : ''}${className ? ` ${className}` : ''}`;

  if (!href) {
    return (
      <Box
        className={combinedClass}
        onMouseEnter={onMouseEnter}
        onMouseLeave={onMouseLeave}
        onClick={onClick}
      >
        {children}
      </Box>
    );
  }

  return (
    <Box
      component={RouterLink as React.ElementType}
      {...{ to: href }}
      className={combinedClass}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
    >
      {children}
    </Box>
  );
}
