import { Tag, Tooltip } from '@jamiehales/ui-kit/mostlymaking';
import type { ExplorerModel } from '../lib/api';
import { SHARED_FIELDS } from './metadata/fieldDefs';
import styles from './Explorer.module.css';

type Source = 'set' | 'inherited' | 'detected';

const SOURCE_LABEL: Record<Source, string> = {
  set: 'Set',
  inherited: 'Inherited',
  detected: 'Detected',
};

const SOURCE_TAG = { set: 'default', inherited: 'neutral', detected: 'outline' } as const;

interface Row {
  key: string;
  label: string;
  value: string;
  tone?: 'supported' | 'unsupported';
  source: Source | null;
  rule: string | null;
}

function displayValue(key: string, value: unknown): string | null {
  if (key === 'supported') {
    if (value == null) return null;
    return value ? 'Supported' : 'Unsupported';
  }
  if (key === 'raftHeightMm') {
    return typeof value === 'number' && Number.isFinite(value) ? `${value} mm` : null;
  }
  return typeof value === 'string' && value !== '' ? value : null;
}

/** The fields with a value, each with where it came from: the model's own entry, a rule, the
 *  folder, or (for supported only) detection from the geometry. */
function metaRows(model: ExplorerModel): Row[] {
  const resolved = (model.resolvedMetadata ?? {}) as unknown as Record<string, unknown>;
  const local = new Set(model.localFields ?? []);
  const rows: Row[] = [];
  for (const field of SHARED_FIELDS) {
    const ruleKey = (field.yamlName ?? field.key).toLowerCase();
    let raw = resolved[field.key];
    let source: Source | null = local.has(field.key) ? 'set' : 'inherited';
    let rule = source === 'set' ? null : (model.ruleConfigs?.[ruleKey] ?? null);
    if (field.key === 'supported' && raw == null && model.detectedSupported != null) {
      raw = model.detectedSupported;
      source = 'detected';
      rule = null;
    }
    const value = displayValue(field.key, raw);
    if (value == null) continue;
    rows.push({
      key: field.key,
      label: field.label,
      value,
      tone: field.key === 'supported' ? (raw ? 'supported' : 'unsupported') : undefined,
      source: rule ? null : source,
      rule,
    });
  }
  return rows;
}

/** A model's metadata as label, value and source rows, matching the folder cards. */
export default function ModelMetaList({ model }: { model: ExplorerModel }) {
  const rows = metaRows(model);
  if (rows.length === 0) return null;

  return (
    <dl className={`${styles.folderMeta} ${styles.modelMeta}`}>
      {rows.map((row) => (
        <div key={row.key} className={styles.metaRow}>
          <dt>{row.label}</dt>
          <dd className={styles.metaValue} data-tone={row.tone}>
            {row.value}
          </dd>
          <dd>
            {row.rule ? (
              <Tooltip side="top" content={<pre className={styles.rulePreview}>{row.rule}</pre>}>
                <button
                  type="button"
                  className={styles.ruleButton}
                  aria-label={`${row.label} comes from a rule: ${row.rule}`}
                  onClick={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                  }}
                >
                  Rule
                </button>
              </Tooltip>
            ) : (
              row.source && (
                <Tag variant={SOURCE_TAG[row.source]} className={styles.sourceTag}>
                  {SOURCE_LABEL[row.source]}
                </Tag>
              )
            )}
          </dd>
        </div>
      ))}
    </dl>
  );
}
