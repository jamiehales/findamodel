import { Input } from '@jamiehales/ui-kit/mostlymaking';

interface Props {
  id?: string;
  value: string | null;
  /** Shown as the placeholder when nothing is set here. */
  inheritedValue?: string | null;
  onChange: (value: string | null) => void;
}

export default function MetadataTextField({ id, value, inheritedValue, onChange }: Props) {
  return (
    <Input
      id={id}
      value={value ?? ''}
      placeholder={inheritedValue ?? undefined}
      onChange={(e) => onChange(e.target.value || null)}
    />
  );
}
