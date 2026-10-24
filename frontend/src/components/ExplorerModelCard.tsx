import { memo } from 'react';
import { RefreshCw } from 'lucide-react';
import { IconButton, Spinner, Tag, Tooltip } from '@jamiehales/ui-kit/mostlymaking';
import type { ExplorerModel } from '../lib/api';
import { useIndexModel, useIsModelIndexing } from '../lib/queries';
import { formatBytes } from '../lib/utils';
import cardStyles from './ModelCard.module.css';
import styles from './Explorer.module.css';

interface Props {
  model: ExplorerModel;
}

/** A model file the indexer hasn't processed yet: no link, and a button to index it. */
function ExplorerModelCard({ model }: Props) {
  const indexModel = useIndexModel(model.relativePath);
  const indexingState = useIsModelIndexing(model.relativePath);
  const name = model.resolvedMetadata?.modelName ?? model.fileName.replace(/\.[^.]+$/, '');

  return (
    <div className={`mm-card ${cardStyles.card} ${styles.unindexed}`}>
      <div className={`mm-card-image ${cardStyles.image}`}>
        <span className={cardStyles.placeholder} aria-hidden="true">
          {model.fileType.toUpperCase()}
        </span>
      </div>
      <div className={cardStyles.body}>
        <span className={styles.unindexedName}>{name}</span>
        <span className={cardStyles.file}>
          {model.fileName}
          {model.fileSize != null ? ` · ${formatBytes(model.fileSize)}` : ''}
        </span>
        <div className={cardStyles.tags}>
          <Tag variant="outline">Not indexed</Tag>
        </div>
      </div>
      <div className={cardStyles.actions} data-pinned="">
        <Tooltip
          content={
            indexingState === 'running'
              ? 'Indexing'
              : indexingState === 'queued'
                ? 'Queued'
                : 'Index model'
          }
        >
          <IconButton
            size="sm"
            aria-label={`Index ${name}`}
            disabled={indexingState !== null}
            onClick={() => indexModel.mutate()}
          >
            {indexingState === 'running' ? (
              <Spinner label="Indexing" />
            ) : (
              <RefreshCw size={16} strokeWidth={1.5} />
            )}
          </IconButton>
        </Tooltip>
      </div>
    </div>
  );
}

export default memo(ExplorerModelCard);
