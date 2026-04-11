import AppBar from '@mui/material/AppBar';
import Box from '@mui/material/Box';
import IconButton from '@mui/material/IconButton';
import Menu from '@mui/material/Menu';
import MenuItem from '@mui/material/MenuItem';
import Toolbar from '@mui/material/Toolbar';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';
import SettingsRoundedIcon from '@mui/icons-material/SettingsRounded';
import { useRef, useState, type MouseEvent as ReactMouseEvent } from 'react';
import { NavLink, Link, useMatch } from 'react-router-dom';
import { useActivePrintingList } from '../lib/queries';
import IndexerStatus from './IndexerStatus';
import styles from './NavBar.module.css';

export default function NavBar() {
  const { data: activeList } = useActivePrintingList();
  const totalCount = activeList?.items.reduce((a, i) => a + i.quantity, 0) ?? 0;
  const printingListMatch = useMatch('/printing-list/*');
  const printingListsMatch = useMatch('/printing-lists');
  const settingsMatch = useMatch('/settings');
  const printingActive = !!(printingListMatch || printingListsMatch);
  const [printingAnchorEl, setPrintingAnchorEl] = useState<HTMLElement | null>(null);
  const printingGroupRef = useRef<HTMLDivElement | null>(null);
  const printingMenuPaperRef = useRef<HTMLDivElement | null>(null);

  function isInPrintingRegion(node: Node | null): boolean {
    if (!node) return false;
    return (
      !!printingGroupRef.current?.contains(node) || !!printingMenuPaperRef.current?.contains(node)
    );
  }

  function openPrintingMenu() {
    setPrintingAnchorEl(printingGroupRef.current);
  }

  function closePrintingMenuImmediately() {
    setPrintingAnchorEl(null);
  }

  function closePrintingMenuIfOutside(event: ReactMouseEvent<HTMLElement>) {
    const nextTarget = event.relatedTarget as Node | null;
    if (!isInPrintingRegion(nextTarget)) {
      closePrintingMenuImmediately();
    }
  }

  const printingMenuOpen = Boolean(printingAnchorEl);

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
          <Box
            ref={printingGroupRef}
            className={`${styles.printingGroup}${printingMenuOpen ? ` ${styles.printingOpen}` : ''}`}
            onMouseEnter={openPrintingMenu}
            onMouseLeave={closePrintingMenuIfOutside}
          >
            <Link
              to="/printing-list/active"
              className={`${styles.navLink}${printingActive ? ` ${styles.navLinkActive}` : ''}`}
              onMouseEnter={openPrintingMenu}
              onClick={closePrintingMenuImmediately}
            >
              Printing
              {totalCount > 0 && <span className={styles.navBadge}>{totalCount}</span>}
            </Link>
            <Menu
              anchorEl={printingAnchorEl}
              open={printingMenuOpen}
              onClose={closePrintingMenuImmediately}
              disableScrollLock
              hideBackdrop
              disableAutoFocusItem
              sx={{ pointerEvents: 'none' }}
              anchorOrigin={{ vertical: 'bottom', horizontal: 'center' }}
              transformOrigin={{ vertical: 'top', horizontal: 'center' }}
              MenuListProps={{
                dense: true,
                onMouseEnter: openPrintingMenu,
                onMouseLeave: closePrintingMenuIfOutside,
              }}
              slotProps={{
                paper: {
                  ref: printingMenuPaperRef,
                  className: styles.printingMenuPaper,
                  onMouseEnter: openPrintingMenu,
                  onMouseLeave: closePrintingMenuIfOutside,
                  sx: { pointerEvents: 'auto' },
                },
              }}
            >
              <MenuItem
                component={Link}
                to="/printing-lists"
                onClick={closePrintingMenuImmediately}
                className={styles.printingManageItem}
              >
                Manage lists
              </MenuItem>
            </Menu>
          </Box>
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
