import { useId, useState } from 'react';
import { X } from 'lucide-react';
import { Input, Tag } from '@jamiehales/ui-kit/mostlymaking';
import styles from './MetadataForm.module.css';

interface Props {
  id?: string;
  localTags: string[] | null;
  inheritedTags: string[] | null;
  tagOptions: string[];
  onChange: (tags: string[] | null) => void;
}

/** Removable tags plus a field that suggests known tags; Enter or comma adds one. */
export default function TagEditor({ id, localTags, inheritedTags, tagOptions, onChange }: Props) {
  const [draft, setDraft] = useState('');
  const listId = useId();
  const local = localTags ?? [];
  const inherited = (inheritedTags ?? []).filter((t) => !local.includes(t));
  const suggestions = tagOptions.filter((t) => !local.includes(t));

  function add(raw: string) {
    const tag = raw.trim();
    setDraft('');
    if (!tag || local.includes(tag)) return;
    onChange([...local, tag]);
  }

  function remove(tag: string) {
    const next = local.filter((t) => t !== tag);
    onChange(next.length > 0 ? next : null);
  }

  return (
    <div className={styles.tagEditor}>
      {(local.length > 0 || inherited.length > 0) && (
        <div className={styles.tags}>
          {local.map((tag) => (
            <Tag key={tag} asChild>
              <button
                type="button"
                className={styles.removable}
                onClick={() => remove(tag)}
                aria-label={`Remove tag ${tag}`}
              >
                {tag}
                <X size={12} strokeWidth={2} aria-hidden="true" />
              </button>
            </Tag>
          ))}
          {inherited.map((tag) => (
            <Tag key={`inherited-${tag}`} variant="outline" title="Inherited">
              {tag}
            </Tag>
          ))}
        </div>
      )}
      <Input
        id={id}
        list={listId}
        value={draft}
        placeholder="Add tag"
        onChange={(e) => {
          const value = e.target.value;
          if (value.endsWith(',')) add(value.slice(0, -1));
          else setDraft(value);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            add(draft);
          } else if (e.key === 'Backspace' && draft === '' && local.length > 0) {
            remove(local[local.length - 1]);
          }
        }}
        onBlur={() => add(draft)}
      />
      <datalist id={listId}>
        {suggestions.map((t) => (
          <option key={t} value={t} />
        ))}
      </datalist>
    </div>
  );
}
