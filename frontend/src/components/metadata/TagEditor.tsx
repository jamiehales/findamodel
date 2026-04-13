import Autocomplete from '@mui/material/Autocomplete';
import Chip from '@mui/material/Chip';
import Stack from '@mui/material/Stack';
import TextField from '@mui/material/TextField';
import styles from './TagEditor.module.css';

interface Props {
  localTags: string[] | null;
  inheritedTags: string[] | null;
  aiTags?: string[] | null;
  tagOptions: string[];
  onChange: (tags: string[] | null) => void;
}

export default function TagEditor({
  localTags,
  inheritedTags,
  aiTags,
  tagOptions,
  onChange,
}: Props) {
  const local = localTags ?? [];
  const inherited = (inheritedTags ?? []).filter((t) => !local.includes(t));
  const ai = (aiTags ?? []).filter((t) => !local.includes(t) && !inherited.includes(t));

  const options = tagOptions.filter((t) => !local.includes(t));

  function handleChange(_: React.SyntheticEvent, newValue: string[]) {
    onChange(newValue.length > 0 ? newValue : null);
  }

  return (
    <Stack>
      <Autocomplete
        multiple
        freeSolo
        size="small"
        value={local}
        options={options}
        onChange={handleChange}
        renderInput={(params) => (
          <TextField
            {...params}
            size="small"
            InputProps={{
              ...params.InputProps,
              startAdornment: (
                <>
                  {params.InputProps.startAdornment}
                  {inherited.map((tag) => (
                    <Chip
                      key={`inherited-${tag}`}
                      label={tag}
                      size="small"
                      variant="outlined"
                      className={styles.inheritedChip}
                    />
                  ))}
                  {ai.map((tag) => (
                    <Chip
                      key={`ai-${tag}`}
                      label={`AI: ${tag}`}
                      size="small"
                      variant="outlined"
                      className={styles.aiChip}
                    />
                  ))}
                </>
              ),
            }}
            placeholder={
              local.length === 0 && inherited.length === 0 && ai.length === 0
                ? 'Add tags…'
                : undefined
            }
          />
        )}
      />
    </Stack>
  );
}
