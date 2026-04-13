import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import Autocomplete from '@mui/material/Autocomplete';
import Checkbox from '@mui/material/Checkbox';
import FormControlLabel from '@mui/material/FormControlLabel';
import Grid from '@mui/material/Grid';
import TextField from '@mui/material/TextField';
import type { FilterOptions, ModelFilter } from '../lib/api';
import styles from './ModelFilters.module.css';

function sortCaseInsensitive(values: string[]): string[] {
  return [...values].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: 'base' }));
}

interface Props {
  value: ModelFilter;
  onChange: (f: ModelFilter) => void;
  options: FilterOptions;
  modelNameOptions: string[];
  modelNameOptionsLoading: boolean;
  modelNameValue: string;
  onModelNameInputChange: (value: string) => void;
  onModelNameChange: (value: string) => void;
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
  const sortedChoices = useMemo(() => sortCaseInsensitive(choices), [choices]);
  const isEmpty = choices.length === 0;
  const isDisabled = !alwaysVisible && isEmpty;
  return (
    <Grid size={1}>
      <Autocomplete
        multiple
        size="small"
        options={sortedChoices}
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

export default function ModelFilters({
  value,
  onChange,
  options,
  modelNameOptions,
  modelNameOptionsLoading,
  modelNameValue,
  onModelNameInputChange,
  onModelNameChange,
}: Props) {
  const [searchInput, setSearchInput] = useState(value.search);
  const [modelNameInput, setModelNameInput] = useState('');
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const sortedModelNameOptions = useMemo(
    () => sortCaseInsensitive(modelNameOptions),
    [modelNameOptions],
  );

  // Keep free-text search in sync with query params
  useEffect(() => {
    setSearchInput(value.search);
  }, [value.search]);

  useEffect(() => {
    setModelNameInput(modelNameValue);
  }, [modelNameValue]);

  useEffect(
    () => () => {
      if (debounceRef.current) window.clearTimeout(debounceRef.current);
    },
    [],
  );

  const handleSearchChange = useCallback(
    (raw: string) => {
      setSearchInput(raw);
      if (debounceRef.current) window.clearTimeout(debounceRef.current);
      debounceRef.current = window.setTimeout(() => {
        onChange({ ...value, search: raw });
      }, 250);
    },
    [onChange, value],
  );

  const handleModelNameInputChange = useCallback(
    (raw: string) => {
      setModelNameInput(raw);
      onModelNameInputChange(raw);
    },
    [onModelNameInputChange],
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
    <Grid
      container
      columns={{ xs: 1, sm: 2, md: 3, lg: 4 }}
      spacing={1.5}
      alignItems="center"
      className={styles.grid}
    >
      <Grid size={{ xs: 1, sm: 2, md: 3, lg: 4 }}>
        <TextField
          fullWidth
          size="small"
          label="Search"
          value={searchInput}
          onChange={(event) => handleSearchChange(event.target.value)}
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
      <Grid size={1}>
        <Autocomplete
          options={sortedModelNameOptions}
          value={modelNameValue || null}
          inputValue={modelNameInput}
          onInputChange={(_event, nextInputValue) => handleModelNameInputChange(nextInputValue)}
          onChange={(_event, selectedValue) => {
            const next = selectedValue ?? '';
            setModelNameInput(next);
            onModelNameChange(next);
          }}
          loading={modelNameOptionsLoading}
          size="small"
          freeSolo={false}
          clearOnBlur={false}
          renderInput={(params) => (
            <TextField {...params} fullWidth size="small" label="Model name" />
          )}
        />
      </Grid>
      <MultiSelect
        label="Tags"
        field="tags"
        choices={options.tags}
        value={value.tags}
        onChange={handleMultiChange}
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
      <Grid size={{ xs: 1, sm: 2, md: 1, lg: 1 }}>
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
