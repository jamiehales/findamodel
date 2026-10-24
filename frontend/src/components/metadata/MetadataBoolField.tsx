import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@jamiehales/ui-kit/mostlymaking';

const UNSET = '__unset__';

interface Props {
  id?: string;
  value: boolean | null;
  inheritedValue?: boolean | null;
  onChange: (value: boolean | null) => void;
}

export default function MetadataBoolField({ id, value, inheritedValue, onChange }: Props) {
  const unsetLabel =
    inheritedValue != null ? `Not set (${inheritedValue ? 'Yes' : 'No'})` : 'Not set';
  return (
    <Select
      value={value === null ? UNSET : String(value)}
      onValueChange={(v) => onChange(v === UNSET ? null : v === 'true')}
    >
      <SelectTrigger id={id}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={UNSET}>{unsetLabel}</SelectItem>
        <SelectItem value="true">Yes</SelectItem>
        <SelectItem value="false">No</SelectItem>
      </SelectContent>
    </Select>
  );
}
