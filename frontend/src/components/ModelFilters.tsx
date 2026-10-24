import { useMemo } from 'react';
import {
  Chip,
  ChipGroup,
  MultiSelect,
  SegmentedControl,
  SegmentedControlItem,
} from '@jamiehales/ui-kit/mostlymaking';
import type { FilterOptions, ModelFilter } from '../lib/api';
import styles from './ModelFilters.module.css';

type ListField = 'creator' | 'collection' | 'subcollection' | 'category' | 'type' | 'tags';
type ChipField = 'fileType' | 'material';

function sortCaseInsensitive(values: string[]): string[] {
  return [...values].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: 'base' }));
}

interface Props {
  value: ModelFilter;
  onChange: (f: ModelFilter) => void;
  options: FilterOptions;
  /** `sidebar` beside the grid on wide screens; `sheet` inside the phone filter sheet. */
  variant?: 'sidebar' | 'sheet';
}

const LISTS: { field: ListField; label: string; options: keyof FilterOptions }[] = [
  { field: 'creator', label: 'Creator', options: 'creators' },
  { field: 'collection', label: 'Collection', options: 'collections' },
  { field: 'subcollection', label: 'Subcollection', options: 'subcollections' },
  { field: 'category', label: 'Category', options: 'categories' },
  { field: 'type', label: 'Type', options: 'types' },
  { field: 'tags', label: 'Tags', options: 'tags' },
];

const CHIPS: { field: ChipField; label: string; options: keyof FilterOptions; upper?: boolean }[] =
  [
    { field: 'fileType', label: 'File type', options: 'fileTypes', upper: true },
    { field: 'material', label: 'Material', options: 'materials' },
  ];

function supportedToSegment(value: boolean | null): string {
  return value === null ? 'any' : value ? 'yes' : 'no';
}

/** The filter sidebar on the Models page. */
export default function ModelFilters({ value, onChange, options, variant = 'sidebar' }: Props) {
  const sorted = useMemo(() => {
    const out = {} as Record<keyof FilterOptions, string[]>;
    for (const key of Object.keys(options) as (keyof FilterOptions)[]) {
      out[key] = sortCaseInsensitive(options[key]);
    }
    return out;
  }, [options]);

  return (
    <aside className={variant === 'sheet' ? styles.inSheet : styles.side} aria-label="Filters">
      {LISTS.map(({ field, label, options: key }) => {
        const id = `filter-${variant}-${field}`;
        return (
          <div key={field} className={styles.group}>
            <label htmlFor={id} className="mm-type-label">
              {label}
            </label>
            <MultiSelect
              id={id}
              aria-label={label}
              options={sorted[key]}
              value={value[field]}
              onValueChange={(next) => onChange({ ...value, [field]: next })}
              disabled={sorted[key].length === 0 && value[field].length === 0}
            />
          </div>
        );
      })}

      <hr className={styles.sep} />

      <div className={styles.group}>
        <span className="mm-type-label">Supports</span>
        <SegmentedControl
          aria-label="Supports"
          value={supportedToSegment(value.supported)}
          onValueChange={(next) =>
            onChange({ ...value, supported: next === 'any' ? null : next === 'yes' })
          }
          className={styles.segment}
        >
          <SegmentedControlItem value="any">Any</SegmentedControlItem>
          <SegmentedControlItem value="yes">Yes</SegmentedControlItem>
          <SegmentedControlItem value="no">No</SegmentedControlItem>
        </SegmentedControl>
      </div>

      {CHIPS.map(({ field, label, options: key, upper }) =>
        sorted[key].length > 0 || value[field].length > 0 ? (
          <div key={field} className={styles.group}>
            <span className="mm-type-label" id={`filter-${variant}-${field}`}>
              {label}
            </span>
            <ChipGroup
              type="multiple"
              aria-labelledby={`filter-${variant}-${field}`}
              value={value[field]}
              onValueChange={(next: string[]) => onChange({ ...value, [field]: next })}
            >
              {[...new Set([...value[field], ...sorted[key]])].map((v) => (
                <Chip key={v} value={v}>
                  {upper ? v.toUpperCase() : v}
                </Chip>
              ))}
            </ChipGroup>
          </div>
        ) : null,
      )}
    </aside>
  );
}
