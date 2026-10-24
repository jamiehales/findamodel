import { Tag } from '@jamiehales/ui-kit/mostlymaking';
import styles from './FilterPill.module.css';

export type FilterPillTone = 'standard' | 'supported' | 'unsupported' | 'user';

interface Props {
  label: string;
  tone?: FilterPillTone;
  onClick: (e: React.MouseEvent<HTMLButtonElement>) => void;
}

const VARIANT = {
  standard: 'neutral',
  supported: 'success',
  unsupported: 'outline',
  user: 'default',
} as const;

/** A tag that filters the Models page by its value when clicked. */
export default function FilterPill({ label, tone = 'standard', onClick }: Props) {
  return (
    <Tag asChild variant={VARIANT[tone]}>
      <button type="button" className={styles.pill} onClick={onClick}>
        {label}
      </button>
    </Tag>
  );
}
