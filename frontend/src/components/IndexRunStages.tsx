import type { IndexRunStage, IndexRunStageName } from '../lib/api/indexer';
import styles from './IndexRunStages.module.css';

const STAGE_LABELS: Record<IndexRunStageName, string> = {
  checksum: 'Checksum',
  load: 'Load',
  supports: 'Supports',
  preview: 'Preview',
  hull: 'Hull',
};

function formatAverage(ms: number): string {
  if (ms < 1000) return `${Math.max(1, Math.round(ms))}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

/** Share of a run's processing time spent in each indexing stage. */
export default function IndexRunStages({ stages }: { stages: IndexRunStage[] }) {
  const ran = stages.filter((s) => s.fileCount > 0 && s.totalMs > 0);
  const total = ran.reduce((sum, s) => sum + s.totalMs, 0);
  if (total <= 0) return null;

  const shares = ran.map((s) => ({
    ...s,
    label: STAGE_LABELS[s.stage] ?? s.stage,
    percent: (s.totalMs / total) * 100,
    average: s.totalMs / s.fileCount,
  }));
  const description = shares.map((s) => `${s.label} ${Math.round(s.percent)}%`).join(', ');

  return (
    <div className={styles.stages}>
      <div className={styles.header}>
        <h3 className="mm-type-label">Time by stage</h3>
        <span className={styles.note}>Share of processing time</span>
      </div>
      <div className={styles.bar} role="img" aria-label={`Time by stage: ${description}`}>
        {shares.map((s) => (
          <span
            key={s.stage}
            className={`${styles.segment} ${styles[s.stage]}`}
            style={{ width: `${s.percent}%` }}
          />
        ))}
      </div>
      <ul className={styles.legend}>
        {shares.map((s) => (
          <li key={s.stage}>
            <span className={`${styles.key} ${styles[s.stage]}`} aria-hidden="true" />
            {s.label}
            <span className={styles.note}>
              {Math.round(s.percent)}% · avg {formatAverage(s.average)}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
