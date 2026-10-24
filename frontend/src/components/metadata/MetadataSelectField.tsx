import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@jamiehales/ui-kit/mostlymaking';

// Radix Select can't hold an empty string, so "not set" has its own value.
const UNSET = '__unset__';

interface Props {
  id?: string;
  value: string | null;
  inheritedValue?: string | null;
  options: string[];
  onChange: (value: string | null) => void;
}

export default function MetadataSelectField({
  id,
  value,
  inheritedValue,
  options,
  onChange,
}: Props) {
  const unsetLabel = inheritedValue ? `Not set (${inheritedValue})` : 'Not set';
  return (
    <Select value={value ?? UNSET} onValueChange={(v) => onChange(v === UNSET ? null : v)}>
      <SelectTrigger id={id}>
        <SelectValue placeholder={unsetLabel} />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={UNSET}>{unsetLabel}</SelectItem>
        {options.map((o) => (
          <SelectItem key={o} value={o}>
            {o}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
