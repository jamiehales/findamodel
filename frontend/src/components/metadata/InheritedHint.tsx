import { useState } from 'react';
import IconButton from '@mui/material/IconButton';
import Stack from '@mui/material/Stack';
import Tooltip from '@mui/material/Tooltip';
import Typography from '@mui/material/Typography';

interface Props {
  value?: string | boolean | number | null;
  /** Folder editor: YAML snippet for an inherited rule */
  inheritedRule?: string | null;
  className?: string;
  hintClassName?: string;
  copyBtnClassName?: string;
}

const CopyIcon = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
    <path d="M16 1H4c-1.1 0-2 .9-2 2v14h2V3h12V1zm3 4H8c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h11c1.1 0 2-.9 2-2V7c0-1.1-.9-2-2-2zm0 16H8V7h11v14z" />
  </svg>
);

export default function InheritedHint({
  value,
  inheritedRule,
  className,
  hintClassName,
  copyBtnClassName,
}: Props) {
  const [copied, setCopied] = useState(false);

  if (value == null && !inheritedRule) return null;

  const textToCopy = inheritedRule ?? String(value);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(textToCopy);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy to clipboard:', err);
    }
  };

  return (
    <Stack direction="row" alignItems="flex-start" spacing={1} className={className}>
      <Typography
        variant="caption"
        color="text.secondary"
        component="div"
        className={hintClassName}
      >
        {inheritedRule ? (
          <>
            Inherited rule:
            <br />
            <code
              style={{
                display: 'block',
                marginTop: '4px',
                fontFamily: 'monospace',
                fontSize: '0.75rem',
                whiteSpace: 'pre-wrap',
                wordBreak: 'break-word',
              }}
            >
              {inheritedRule}
            </code>
          </>
        ) : (
          <>Inherited: {String(value)}</>
        )}
      </Typography>
      <Tooltip title={copied ? 'Copied!' : 'Copy to clipboard'}>
        <IconButton size="small" onClick={handleCopy} className={copyBtnClassName}>
          <CopyIcon />
        </IconButton>
      </Tooltip>
    </Stack>
  );
}
