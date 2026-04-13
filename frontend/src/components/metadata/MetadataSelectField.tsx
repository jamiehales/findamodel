import FormControl from '@mui/material/FormControl';
import MenuItem from '@mui/material/MenuItem';
import Select from '@mui/material/Select';

interface Props {
  value: string | null;
  inheritedValue?: string | null;
  options: string[];
  onChange: (value: string | null) => void;
  selectClassName?: string;
}

export default function MetadataSelectField({
  value,
  inheritedValue,
  options,
  onChange,
  selectClassName,
}: Props) {
  return (
    <>
      <FormControl size="small" fullWidth>
        <Select
          displayEmpty
          value={value ?? ''}
          onChange={(e) => onChange(e.target.value || null)}
          className={selectClassName}
          renderValue={(v) =>
            v ? (
              String(v)
            ) : (
              <em style={{ color: 'inherit', opacity: 0.5 }}>
                Not set{inheritedValue ? ` (${inheritedValue})` : ''}
              </em>
            )
          }
        >
          <MenuItem value="">
            <em>Not set</em>
          </MenuItem>
          {options.map((o) => (
            <MenuItem key={o} value={o}>
              {o}
            </MenuItem>
          ))}
        </Select>
      </FormControl>
    </>
  );
}
