import { Link, NavLink, useMatch } from 'react-router-dom';
import { Settings } from 'lucide-react';
import { Button, Switch } from '@jamiehales/ui-kit/mostlymaking';
import { useActivePrintingList } from '../lib/queries';
import { useRenderControls } from './RenderControlsContext';
import IndexerStatus from './IndexerStatus';
import styles from './AppHeader.module.css';

function navClass({ isActive }: { isActive: boolean }) {
  return isActive ? `${styles.nav} ${styles.navActive}` : styles.nav;
}

export default function AppHeader() {
  const { data: activeList } = useActivePrintingList();
  const { showSupports, setShowSupports } = useRenderControls();
  const totalCount = activeList?.items.reduce((a, i) => a + i.quantity, 0) ?? 0;
  const printingListMatch = useMatch('/printing-list/*');
  const printingListsMatch = useMatch('/printing-lists');
  const settingsMatch = useMatch('/settings/*');
  const printingActive = !!(printingListMatch || printingListsMatch);

  return (
    <header className={styles.header}>
      <Link to="/" className={`mm-type-logo ${styles.logo}`}>
        mostlymaking<span className={styles.dot}>.</span>
        <span className={styles.product}>findamodel</span>
      </Link>
      <nav className={styles.navs} aria-label="Primary">
        <NavLink to="/" end className={navClass}>
          Models
        </NavLink>
        <NavLink to="/explore" className={navClass}>
          Explore
        </NavLink>
        <Link
          to="/printing-list/active"
          className={navClass({ isActive: printingActive })}
          aria-current={printingActive ? 'page' : undefined}
        >
          Printing
          {totalCount > 0 && <span className={styles.count}>{totalCount}</span>}
        </Link>
      </nav>
      <div className={styles.spacer} />
      <IndexerStatus />
      <Switch checked={showSupports} onCheckedChange={(checked) => setShowSupports(checked)}>
        Supports
      </Switch>
      <Button
        asChild
        variant="ghost"
        icon
        aria-label="Settings"
        className={settingsMatch ? styles.settingsActive : undefined}
      >
        <Link to="/settings" aria-current={settingsMatch ? 'page' : undefined}>
          <Settings size={18} strokeWidth={1.5} />
        </Link>
      </Button>
    </header>
  );
}
