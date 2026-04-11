import FormControl from '@mui/material/FormControl';
import MenuItem from '@mui/material/MenuItem';
import Select from '@mui/material/Select';
import InheritedHint from './InheritedHint';

interface Props {
  value: boolean | null;
  inheritedValue?: boolean | null;
  inheritedRule?: string | null;
  onChange: (value: boolean | null) => void;
  selectClassName?: string;
  hintContainerClassName?: string;
  hintClassName?: string;
  copyBtnClassName?: string;
}

export default function MetadataBoolField({
  value,
  inheritedValue,
  inheritedRule,
  onChange,
  selectClassName,
  hintContainerClassName,
  hintClassName,
  copyBtnClassName,
}: Props) {
  return (
    <>
      <FormControl size="small" fullWidth>
        <Select
          displayEmpty
          value={value === null ? '' : String(value)}
          onChange={(e) => {
            const v = e.target.value;
            onChange(v === '' ? null : v === 'true');
          }}
          className={selectClassName}
          renderValue={(v) =>
            v !== '' ? (
              v === 'true' ? (
                'True'
              ) : (
                'False'
              )
            ) : (
              <em style={{ color: 'inherit', opacity: 0.5 }}>
                Not set{inheritedValue != null ? ` (${String(inheritedValue)})` : ''}
              </em>
            )
          }
        >
          <MenuItem value="">
            <em>Not set</em>
          </MenuItem>
          <MenuItem value="true">True</MenuItem>
          <MenuItem value="false">False</MenuItem>
        </Select>
      </FormControl>
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
