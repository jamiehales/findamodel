import type { ReactNode } from 'react';
import styles from './PageLayout.module.css';

type PageVariant = 'full' | 'medium' | 'narrow';

interface PageLayoutProps {
  variant?: PageVariant;
  children: ReactNode;
  className?: string;
}

/** The padded content area under the header. */
export default function PageLayout({ variant = 'full', children, className }: PageLayoutProps) {
  return (
    <main className={[styles.page, styles[variant], className].filter(Boolean).join(' ')}>
      {children}
    </main>
  );
}

interface PageHeaderProps {
  title: ReactNode;
  /** Shown above the title, e.g. a breadcrumb. */
  before?: ReactNode;
  /** Tags or status beside the title. */
  meta?: ReactNode;
  /** Buttons on the right. */
  actions?: ReactNode;
}

export function PageHeader({ title, before, meta, actions }: PageHeaderProps) {
  return (
    <div className={styles.header}>
      <div className={styles.titleGroup}>
        {before}
        <div className={styles.titleRow}>
          <h1 className={`mm-type-title-lg ${styles.title}`}>{title}</h1>
          {meta}
        </div>
      </div>
      {actions ? <div className={styles.actions}>{actions}</div> : null}
    </div>
  );
}
