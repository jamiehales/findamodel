import { IconButton, Badge } from '@mui/material';
import { Link, useMatch } from 'react-router-dom';
import { useIndexerStatus } from '../lib/queries';
import styles from './IndexerStatus.module.css';

export default function IndexerStatus() {
  const { data: status } = useIndexerStatus();
  const indexingMatch = useMatch('/indexing');

  const isRunning = !!status?.isRunning;
  const hasQueue = !!status?.queue.length;
  const isActive = !!(status?.isRunning || hasQueue || indexingMatch);
  const totalCount = (status?.isRunning ? 1 : 0) + (status?.queue.length ?? 0);

  return (
    <IconButton
      component={Link}
      to="/indexing"
      size="small"
      className={`${styles.iconButton} ${isActive ? styles.iconButtonActive : ''}`}
    >
      <Badge badgeContent={totalCount > 1 ? totalCount : 0} classes={{ badge: styles.badge }}>
        <svg
          width="18"
          height="18"
          viewBox="0 0 24 24"
          fill="currentColor"
          className={isRunning ? styles.iconSpin : undefined}
        >
          {isRunning ? (
            <path d="M17.65 6.35A7.958 7.958 0 0 0 12 4c-4.42 0-7.99 3.58-7.99 8s3.57 8 7.99 8c3.73 0 6.84-2.55 7.73-6h-2.08A5.99 5.99 0 0 1 12 18c-3.31 0-6-2.69-6-6s2.69-6 6-6c1.66 0 3.14.69 4.22 1.78L13 11h7V4l-2.35 2.35z" />
          ) : (
            <path d="M4 6h16v2H4V6zm0 5h16v2H4v-2zm0 5h10v2H4v-2z" />
          )}
        </svg>
      </Badge>
    </IconButton>
  );
}
