import { useEffect, useState } from 'react';
import { Spinner } from '@jamiehales/ui-kit/mostlymaking';
import { useRulePreview } from '../../lib/queries';
import styles from './RulePreview.module.css';

interface Props {
  /** Folder the rule is set on. */
  path: string;
  /** YAML field name, e.g. "model_name". */
  field: string;
  rule: string;
  enabled: boolean;
}

const DEBOUNCE_MS = 300;
const numberFormat = new Intl.NumberFormat();

function displayValue(field: string, value: string): string {
  if (field === 'supported') return value === 'true' ? 'Supported' : 'Unsupported';
  return value;
}

function useDebounced<T>(value: T, delayMs: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), delayMs);
    return () => clearTimeout(timer);
  }, [value, delayMs]);
  return debounced;
}

/** What a folder rule produces for the models under the folder, updated as the rule is typed. */
export default function RulePreview({ path, field, rule, enabled }: Props) {
  const debouncedRule = useDebounced(rule.trim(), DEBOUNCE_MS);
  const { data, isFetching, isError } = useRulePreview(path, field, debouncedRule, enabled);

  if (!enabled || debouncedRule === '') return null;
  if (isError) return <p className={styles.error}>Couldn't preview this rule.</p>;
  if (!data) {
    return (
      <div className={styles.preview}>
        <Spinner label="Previewing rule" />
      </div>
    );
  }
  if (data.error) return <p className={styles.error}>{data.error}</p>;

  return (
    <div className={styles.preview} aria-live="polite" aria-busy={isFetching}>
      <p className={`mm-type-small ${styles.summary}`}>
        {data.modelCount === 0
          ? 'No indexed models in this folder'
          : `Matches ${numberFormat.format(data.matchedCount)} of ${numberFormat.format(data.modelCount)}${data.truncated ? '+' : ''} models`}
        {isFetching && <Spinner label="Updating preview" />}
      </p>

      {data.values.length > 0 && (
        <ul className={styles.values} aria-label="Values">
          {data.values.map(({ value, count }) => (
            <li key={value} className={styles.value}>
              <span className={styles.valueText}>{displayValue(field, value)}</span>
              <span className={styles.count}>{numberFormat.format(count)}</span>
            </li>
          ))}
        </ul>
      )}

      {data.samples.length > 0 && (
        <ul className={styles.samples} aria-label="Examples">
          {data.samples.map((sample) => (
            <li key={sample.relativePath} className={styles.sample}>
              <span className={styles.samplePath} title={sample.relativePath}>
                <bdi>{sample.relativePath}</bdi>
              </span>
              <span className={sample.values.length > 0 ? styles.sampleValue : styles.noMatch}>
                {sample.values.length > 0
                  ? sample.values.map((v) => displayValue(field, v)).join(', ')
                  : 'No match'}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
