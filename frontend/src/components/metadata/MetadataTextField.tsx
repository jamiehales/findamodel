import TextField from '@mui/material/TextField';

interface Props {
  value: string | null;
  inheritedValue?: string | null;
  onChange: (value: string | null) => void;
  inputClassName?: string;
}

export default function MetadataTextField({
  value,
  inheritedValue,
  onChange,
  inputClassName,
}: Props) {
  return (
    <>
      <TextField
        size="small"
        fullWidth
        value={value ?? ''}
        placeholder={inheritedValue ?? undefined}
        onChange={(e) => onChange(e.target.value || null)}
        InputProps={{ className: inputClassName }}
      />
    </>
  );
}
