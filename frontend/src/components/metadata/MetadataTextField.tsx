import TextField from '@mui/material/TextField';
import InheritedHint from './InheritedHint';

interface Props {
  value: string | null;
  inheritedValue?: string | null;
  inheritedRule?: string | null;
  onChange: (value: string | null) => void;
  inputClassName?: string;
  hintContainerClassName?: string;
  hintClassName?: string;
  copyBtnClassName?: string;
}

export default function MetadataTextField({
  value,
  inheritedValue,
  inheritedRule,
  onChange,
  inputClassName,
  hintContainerClassName,
  hintClassName,
  copyBtnClassName,
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
      <InheritedHint
        value={inheritedValue}
        inheritedRule={inheritedRule}
        className={hintContainerClassName}
        hintClassName={hintClassName}
        copyBtnClassName={copyBtnClassName}
      />
    </>
  );
}
