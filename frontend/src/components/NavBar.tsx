import AppBar from '@mui/material/AppBar';
import IconButton from '@mui/material/IconButton';
import Toolbar from '@mui/material/Toolbar';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';
import SettingsRoundedIcon from '@mui/icons-material/SettingsRounded';
import { NavLink, Link, useMatch } from 'react-router-dom';
import { useActivePrintingList } from '../lib/queries';
import IndexerStatus from './IndexerStatus';
import styles from './NavBar.module.css';

export default function NavBar() {
  const { data: activeList } = useActivePrintingList();
  const totalCount = activeList?.items.reduce((a, i) => a + i.quantity, 0) ?? 0;
  const printingListMatch = useMatch('/printing-list/*');
  const printingListsMatch = useMatch('/printing-lists');
  const settingsMatch = useMatch('/settings/*');
  const printingActive = !!(printingListMatch || printingListsMatch);

  return (
    <AppBar position="sticky">
      <Toolbar>
        <Typography
          component={Link}
          to="/"
          sx={{
            fontSize: '1.15rem',
            fontWeight: 700,
            letterSpacing: '-0.03em',
            color: '#e2e8f0',
            textDecoration: 'none',
            lineHeight: 1,
          }}
        >
          find<span className={styles.brandA}>a</span>model
        </Typography>
        <Stack component="nav" direction="row" spacing={0.5} className={styles.navLinks}>
          <NavLink
            to="/"
            end
            className={({ isActive }) =>
              `${styles.navLink}${isActive ? ` ${styles.navLinkActive}` : ''}`
            }
          >
            Models
          </NavLink>
          <NavLink
            to="/explore"
            className={({ isActive }) =>
              `${styles.navLink}${isActive ? ` ${styles.navLinkActive}` : ''}`
            }
          >
            Explore
          </NavLink>
          <Link
            to="/printing-list/active"
            className={`${styles.navLink}${printingActive ? ` ${styles.navLinkActive}` : ''}`}
          >
            Printing
            {totalCount > 0 && <span className={styles.navBadge}>{totalCount}</span>}
          </Link>
        </Stack>
        <Stack direction="row" spacing={0.5} alignItems="center" className={styles.actionsRight}>
          <IconButton
            component={Link}
            to="/settings"
            size="small"
            className={`${styles.settingsIconButton}${settingsMatch ? ` ${styles.settingsIconButtonActive}` : ''}`}
          >
            <SettingsRoundedIcon fontSize="small" />
          </IconButton>
          <IndexerStatus />
        </Stack>
      </Toolbar>
    </AppBar>
  );
}
