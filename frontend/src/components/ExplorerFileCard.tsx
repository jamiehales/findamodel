import { useState } from 'react';
import ArticleOutlinedIcon from '@mui/icons-material/ArticleOutlined';
import CloseOutlinedIcon from '@mui/icons-material/CloseOutlined';
import Box from '@mui/material/Box';
import CircularProgress from '@mui/material/CircularProgress';
import Dialog from '@mui/material/Dialog';
import DialogContent from '@mui/material/DialogContent';
import IconButton from '@mui/material/IconButton';
import Typography from '@mui/material/Typography';
import type { ExplorerFile } from '../lib/api';
import { explorerFileUrl } from '../lib/api';
import { formatBytes } from '../lib/utils';
import { useExplorerFileText } from '../lib/queries';
import AppCard from './AppCard';
import styles from './ExplorerFileCard.module.css';
import { appColors } from '../theme';

const IMAGE_TYPES = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp']);
const TEXT_TYPES = new Set(['txt', 'md']);
const MAX_TEXT_PREVIEW_BYTES = 512 * 1024;

interface Props {
  file: ExplorerFile;
}

export default function ExplorerFileCard({ file }: Props) {
  const [open, setOpen] = useState(false);

  const fileType = file.fileType.toLowerCase();
  const isImage = IMAGE_TYPES.has(fileType);
  const isText = TEXT_TYPES.has(fileType);

  const badgeColor =
    appColors.fileType[fileType] ??
    ({ bg: 'rgba(148,163,184,0.2)', color: '#cbd5e1' } as { bg: string; color: string });

  const tooLargeForTextPreview = isText && file.fileSize > MAX_TEXT_PREVIEW_BYTES;

  const {
    data: textContent,
    isLoading: textLoading,
    error: textQueryError,
  } = useExplorerFileText(file.relativePath, open && isText && !tooLargeForTextPreview);

  const textError =
    textQueryError instanceof Error
      ? textQueryError.message
      : textQueryError
        ? 'Failed to load text preview'
        : null;

  function handleOpen() {
    setOpen(true);
  }

  return (
    <>
      <AppCard className={styles.card} onClick={handleOpen}>
        {isImage ? (
          <Box
            component="img"
            src={explorerFileUrl(file.relativePath)}
            alt=""
            className={styles.previewImage}
          />
        ) : (
          <Box className={styles.previewText}>
            <ArticleOutlinedIcon className={styles.previewTextIcon} />
            <Typography className={styles.previewTextType}>
              {file.fileType.toUpperCase()}
            </Typography>
          </Box>
        )}

        <Box className={styles.overlay}>
          <span
            className={styles.badge}
            style={{ background: badgeColor.bg, color: badgeColor.color }}
          >
            {file.fileType.toUpperCase()}
          </span>

          <Typography className={styles.name}>{file.fileName}</Typography>
          <Typography className={styles.size}>{formatBytes(file.fileSize)}</Typography>
        </Box>
      </AppCard>

      <Dialog open={open} onClose={() => setOpen(false)} fullWidth maxWidth={isImage ? 'lg' : 'md'}>
        <DialogContent>
          <Box className={styles.dialogTitleRow}>
            <h3 className={styles.dialogTitle}>{file.fileName}</h3>
            <IconButton aria-label="Close preview" onClick={() => setOpen(false)}>
              <CloseOutlinedIcon />
            </IconButton>
          </Box>

          <Box className={styles.dialogBody}>
            {isImage && (
              <Box
                component="img"
                src={explorerFileUrl(file.relativePath)}
                alt={file.fileName}
                className={styles.imageDialog}
              />
            )}

            {isText && tooLargeForTextPreview && (
              <Typography className={styles.textInfo}>
                {`File too large to preview (${formatBytes(file.fileSize)}).`}
              </Typography>
            )}

            {isText && !tooLargeForTextPreview && textLoading && <CircularProgress size={20} />}

            {isText && !tooLargeForTextPreview && textError && (
              <Typography className={styles.textInfo}>{textError}</Typography>
            )}

            {isText &&
              !tooLargeForTextPreview &&
              !textLoading &&
              !textError &&
              textContent !== undefined && <pre className={styles.textContent}>{textContent}</pre>}
          </Box>
        </DialogContent>
      </Dialog>
    </>
  );
}
