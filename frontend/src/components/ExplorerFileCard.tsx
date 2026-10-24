import { useState } from 'react';
import { Download, File, Image } from 'lucide-react';
import { IconButton, Spinner, Tag } from '@jamiehales/ui-kit/mostlymaking';
import type { ExplorerFile } from '../lib/api';
import { explorerFileUrl } from '../lib/api';
import { formatBytes } from '../lib/utils';
import { useExplorerFileText } from '../lib/queries';
import AppDialog from './AppDialog';
import styles from './Explorer.module.css';

const IMAGE_TYPES = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp']);
const TEXT_TYPES = new Set(['txt', 'md']);
const MAX_TEXT_PREVIEW_BYTES = 512 * 1024;

interface Props {
  file: ExplorerFile;
}

/** One row in the Files list. Images and text files open a preview. */
export default function ExplorerFileRow({ file }: Props) {
  const [open, setOpen] = useState(false);

  const fileType = file.fileType.toLowerCase();
  const isImage = IMAGE_TYPES.has(fileType);
  const isText = TEXT_TYPES.has(fileType);
  const canPreview = isImage || isText;
  const tooLargeForTextPreview = isText && file.fileSize > MAX_TEXT_PREVIEW_BYTES;
  const url = explorerFileUrl(file.relativePath);

  const {
    data: textContent,
    isLoading: textLoading,
    error: textQueryError,
  } = useExplorerFileText(file.relativePath, open && isText && !tooLargeForTextPreview);

  const Icon = isImage ? Image : File;

  return (
    <>
      <div className={styles.fileRow}>
        <Icon size={16} strokeWidth={1.5} aria-hidden="true" className={styles.fileIcon} />
        {canPreview ? (
          <button type="button" className={styles.fileName} onClick={() => setOpen(true)}>
            {file.fileName}
          </button>
        ) : (
          <span className={styles.fileName}>{file.fileName}</span>
        )}
        <Tag variant="neutral">{file.fileType.toUpperCase()}</Tag>
        <span className={styles.fileSize}>{formatBytes(file.fileSize)}</span>
        <IconButton asChild variant="ghost" size="sm" aria-label={`Download ${file.fileName}`}>
          <a href={url} download={file.fileName}>
            <Download size={16} strokeWidth={1.5} />
          </a>
        </IconButton>
      </div>

      {canPreview && (
        <AppDialog
          open={open}
          onClose={() => setOpen(false)}
          title={file.fileName}
          size={isImage ? 'lg' : 'md'}
        >
          {isImage && <img src={url} alt={file.fileName} className={styles.previewImage} />}
          {isText && tooLargeForTextPreview && (
            <p className={styles.meta}>Too large to preview ({formatBytes(file.fileSize)}).</p>
          )}
          {isText && !tooLargeForTextPreview && textLoading && <Spinner />}
          {isText && !tooLargeForTextPreview && textQueryError && (
            <p className={styles.error}>Couldn't load the preview.</p>
          )}
          {isText && !tooLargeForTextPreview && !textLoading && textContent !== undefined && (
            <pre className={styles.previewText}>{textContent}</pre>
          )}
        </AppDialog>
      )}
    </>
  );
}
