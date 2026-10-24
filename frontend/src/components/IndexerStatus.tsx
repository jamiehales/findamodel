import { Link } from 'react-router-dom';
import { ListChecks } from 'lucide-react';
import { Progress, Spinner } from '@jamiehales/ui-kit/mostlymaking';
import { useIndexerStatus } from '../lib/queries';
import styles from './IndexerStatus.module.css';

const numberFormat = new Intl.NumberFormat();

/** Header link to the Indexing page; shows live progress while a run is going. */
export default function IndexerStatus() {
  const { data: status } = useIndexerStatus();
  const current = status?.currentRequest ?? null;
  const queued = status?.queue.length ?? 0;

  if (!status?.isRunning || !current) {
    return (
      <Link to="/indexing" className={styles.link} aria-label="Indexing">
        <ListChecks size={18} strokeWidth={1.5} aria-hidden="true" />
        {queued > 0 && <span className={styles.count}>{queued} queued</span>}
      </Link>
    );
  }

  const total = current.totalFiles;
  const processed = current.processedFiles;
  const label =
    total != null
      ? `Indexing, ${numberFormat.format(processed)} of ${numberFormat.format(total)} files`
      : 'Indexing';

  return (
    <Link to="/indexing" className={styles.link} aria-label={label}>
      <Spinner className={styles.spinner} label="Indexing" />
      <span className={styles.text}>Indexing</span>
      {total != null && total > 0 && (
        <>
          <Progress value={processed} max={total} className={styles.progress} />
          <span className={styles.count}>
            {numberFormat.format(processed)} / {numberFormat.format(total)}
          </span>
        </>
      )}
      {queued > 0 && <span className={styles.count}>+{queued}</span>}
    </Link>
  );
}
