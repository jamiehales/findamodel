import { useCallback, useEffect, useRef, useState } from 'react';
import Autocomplete from '@mui/material/Autocomplete';
import Checkbox from '@mui/material/Checkbox';
import FormControlLabel from '@mui/material/FormControlLabel';
import Grid from '@mui/material/Grid';
import TextField from '@mui/material/TextField';
import type { FilterOptions, ModelFilter } from '../lib/api';
import styles from './ModelFilters.module.css';

interface Props {
  value: ModelFilter;
  onChange: (f: ModelFilter) => void;
  options: FilterOptions;
}

function MultiSelect({
  label,
  field,
  choices,
  value,
  onChange,
  alwaysVisible,
}: {
  label: string;
  field: keyof Pick<
    ModelFilter,
    | 'creator'
    | 'collection'
    | 'subcollection'
    | 'tags'
    | 'category'
    | 'type'
    | 'material'
    | 'fileType'
  >;
  choices: string[];
  value: string[];
  onChange: (field: string, selected: string[]) => void;
  alwaysVisible?: boolean;
}) {
  const isEmpty = choices.length === 0;
  const isDisabled = !alwaysVisible && isEmpty;
  return (
    <Grid size={1}>
      <Autocomplete
        multiple
        size="small"
        options={choices}
        value={value}
        onChange={(_, selected) => onChange(field, selected)}
        disabled={isDisabled}
        renderInput={(params) => <TextField {...params} label={label} />}
      />
    </Grid>
  );
}

// Cycles: null → true → false → null
function nextSupportedState(current: boolean | null): boolean | null {
  if (current === null) return true;
  if (current === true) return false;
  return null;
}

export default function ModelFilters({ value, onChange, options }: Props) {
  const [searchInput, setSearchInput] = useState(value.search);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Keep local search state in sync if parent resets the filter
  useEffect(() => {
    setSearchInput(value.search);
  }, [value.search]);

  const handleSearchChange = useCallback(
    (raw: string) => {
      setSearchInput(raw);
      if (debounceRef.current) clearTimeout(debounceRef.current);
      debounceRef.current = setTimeout(() => {
        onChange({ ...value, search: raw });
      }, 300);
    },
    [value, onChange],
  );

  const handleMultiChange = useCallback(
    (field: string, selected: string[]) => {
      onChange({ ...value, [field]: selected });
    },
    [value, onChange],
  );

  const handleSupportedToggle = useCallback(() => {
    onChange({ ...value, supported: nextSupportedState(value.supported) });
  }, [value, onChange]);

  return (
    <Grid container columns={2} spacing={1.5} alignItems="center" className={styles.grid}>
      <Grid size={2}>
        <TextField
          fullWidth
          size="small"
          label="Search"
          value={searchInput}
          onChange={(e) => handleSearchChange(e.target.value)}
        />
      </Grid>
      <MultiSelect
        label="Creator"
        field="creator"
        choices={options.creators}
        value={value.creator}
        onChange={handleMultiChange}
      />
      <MultiSelect
        label="Collection"
        field="collection"
        choices={options.collections}
        value={value.collection}
        onChange={handleMultiChange}
      />
      <MultiSelect
        label="Subcollection"
        field="subcollection"
        choices={options.subcollections}
        value={value.subcollection}
        onChange={handleMultiChange}
      />
      <MultiSelect
        label="Tags"
        field="tags"
        choices={options.tags}
        value={value.tags}
        onChange={handleMultiChange}
        alwaysVisible
      />
      <MultiSelect
        label="Category"
        field="category"
        choices={options.categories}
        value={value.category}
        onChange={handleMultiChange}
      />
      <MultiSelect
        label="Type"
        field="type"
        choices={options.types}
        value={value.type}
        onChange={handleMultiChange}
      />
      <MultiSelect
        label="Material"
        field="material"
        choices={options.materials}
        value={value.material}
        onChange={handleMultiChange}
      />
      <MultiSelect
        label="File type"
        field="fileType"
        choices={options.fileTypes}
        value={value.fileType}
        onChange={handleMultiChange}
      />
      <Grid size={1}>
        <FormControlLabel
          label="Supported"
          control={
            <Checkbox
              checked={value.supported === true}
              indeterminate={value.supported === null}
              onChange={handleSupportedToggle}
            />
          }
        />
      </Grid>
    </Grid>
  );
}
