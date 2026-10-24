import { Fragment } from 'react';
import { Link } from 'react-router-dom';
import styles from './PathBreadcrumb.module.css';

interface PathBreadcrumbProps {
  /** Forward-slash separated path. The last segment is the current item and isn't a link. */
  path: string;
}

export default function PathBreadcrumb({ path }: PathBreadcrumbProps) {
  const segments = path === '' ? [] : path.split('/');

  return (
    <nav className={styles.crumbs} aria-label="Folder path">
      {segments.length === 0 ? (
        <span className={styles.current} aria-current="page">
          models
        </span>
      ) : (
        <Link to="/explore">models</Link>
      )}
      {segments.map((seg, i) => {
        const segPath = segments.slice(0, i + 1).join('/');
        const isLast = i === segments.length - 1;
        return (
          <Fragment key={segPath}>
            <span aria-hidden="true">/</span>
            {isLast ? (
              <span className={styles.current} aria-current="page">
                {seg}
              </span>
            ) : (
              <Link to={`/explore/${segPath}`}>{seg}</Link>
            )}
          </Fragment>
        );
      })}
    </nav>
  );
}
