import styles from './FilterPill.module.css';

export type FilterPillTone = 'standard' | 'supported' | 'unsupported' | 'user';

interface Props {
  label: string;
  tone?: FilterPillTone;
  onClick: (e: React.MouseEvent<HTMLButtonElement>) => void;
}

export default function FilterPill({ label, tone = 'standard', onClick }: Props) {
  const toneClass =
    tone === 'supported'
      ? styles.supported
      : tone === 'unsupported'
        ? styles.unsupported
        : tone === 'user'
          ? styles.user
          : styles.standard;

  return (
    <button type="button" className={`${styles.pill} ${toneClass}`} onClick={onClick}>
      {label}
    </button>
  );
}
