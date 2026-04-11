import { memo, useState } from 'react';
import Box from '@mui/material/Box';
import Stack from '@mui/material/Stack';
import Tooltip from '@mui/material/Tooltip';
import Typography from '@mui/material/Typography';
import IconButton from '@mui/material/IconButton';
import CircularProgress from '@mui/material/CircularProgress';
import LayersRoundedIcon from '@mui/icons-material/LayersRounded';
import type { ExplorerModel } from '../lib/api';
import { useIndexModel, useIsModelIndexing } from '../lib/queries';
import AppCard from './AppCard';
import CodeTooltip from './CodeTooltip';
import PrintingListControls from './PrintingListControls';
import styles from './ExplorerModelCard.module.css';
import { formatBytes } from '../lib/utils';

function MetaBadge({
  value,
  isRule,
  ruleYaml,
}: {
  value: string;
  isRule: boolean;
  ruleYaml?: string | null;
}) {
  const badge = (
    <Box
      component="span"
      className={`${styles.metaBadge} ${isRule ? styles.metaBadgeRule : styles.metaBadgeValue}`}
    >
      {value}
    </Box>
  );

  if (!isRule || !ruleYaml) return badge;

  return (
    <CodeTooltip code={ruleYaml} placement="top">
      {badge}
    </CodeTooltip>
  );
}

function MetaBadges({
  meta,
  ruleConfigs,
}: {
  meta: ExplorerModel['resolvedMetadata'] & object;
  ruleConfigs: Record<string, string> | null;
}) {
  const entries: { value: string; isRule: boolean; ruleYaml?: string }[] = [];

  if (meta.creator)
    entries.push({
      value: meta.creator,
      isRule: 'creator' in (ruleConfigs ?? {}),
      ruleYaml: ruleConfigs?.creator,
    });
  if (meta.collection)
    entries.push({
      value: meta.collection,
      isRule: 'collection' in (ruleConfigs ?? {}),
      ruleYaml: ruleConfigs?.collection,
    });
  if (meta.category)
    entries.push({
      value: meta.category,
      isRule: 'category' in (ruleConfigs ?? {}),
      ruleYaml: ruleConfigs?.category,
    });
  if (meta.type)
    entries.push({
      value: meta.type,
      isRule: 'type' in (ruleConfigs ?? {}),
      ruleYaml: ruleConfigs?.type,
    });
  if (meta.material)
    entries.push({
      value: meta.material,
      isRule: 'material' in (ruleConfigs ?? {}),
      ruleYaml: ruleConfigs?.material,
    });
  if (meta.supported != null)
    entries.push({ value: meta.supported ? 'Supported' : 'Unsupported', isRule: false });

  if (entries.length === 0) return null;

  return (
    <Stack direction="row" flexWrap="wrap" gap={0.5} className={styles.metaBadges}>
      {entries.map((e, i) => (
        <MetaBadge key={i} value={e.value} isRule={e.isRule} ruleYaml={e.ruleYaml} />
      ))}
    </Stack>
  );
}

interface Props {
  model: ExplorerModel;
  href?: string;
}

function ExplorerModelCard({ model, href }: Props) {
  const fileType = model.fileType.toLowerCase();
  const isNonGeometry = fileType === 'lys' || fileType === 'lyt' || fileType === 'ctb';
  const badgeClass =
    fileType === 'stl'
      ? styles.badgeStl
      : fileType === 'obj'
        ? styles.badgeObj
        : fileType === 'ctb'
          ? styles.badgeCtb
          : fileType === 'lys' || fileType === 'lyt'
            ? styles.badgeLychee
            : styles.badgeDefault;
  const isIndexed = model.id != null;
  const [hovered, setHovered] = useState(false);
  const indexModel = useIndexModel(model.relativePath);
  const indexingState = useIsModelIndexing(model.relativePath);

  return (
    <AppCard
      href={href}
      interactive
      className={`${styles.card}${!isIndexed ? ` ${styles.cardUnindexed}` : ''}`}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {model.previewUrl ? (
        <Box component="img" src={model.previewUrl} alt="" className={styles.preview} />
      ) : isNonGeometry ? (
        <Box className={styles.previewPlaceholder}>
          <LayersRoundedIcon className={styles.previewPlaceholderIcon} />
          <Typography className={styles.previewPlaceholderText}>
            {model.fileType.toUpperCase()}
          </Typography>
        </Box>
      ) : null}

      <Box className={styles.overlay}>
        <span className={`${styles.badge} ${badgeClass}`}>{model.fileType.toUpperCase()}</span>

        <Typography className={styles.name}>
          {model.resolvedMetadata?.modelName ?? model.fileName.replace(/\.[^.]+$/, '')}
        </Typography>

        {model.fileSize != null && (
          <Typography className={styles.size}>{formatBytes(model.fileSize)}</Typography>
        )}

        {!isIndexed && <Typography className={styles.unindexedLabel}>Not yet indexed</Typography>}

        {model.resolvedMetadata && (
          <MetaBadges meta={model.resolvedMetadata} ruleConfigs={model.ruleConfigs} />
        )}
      </Box>

      {model.id && <PrintingListControls modelId={model.id} showButtons={hovered} />}

      {!model.id && (
        <Tooltip
          title={
            indexingState === 'running'
              ? 'Indexing…'
              : indexingState === 'queued'
                ? 'Queued…'
                : 'Index model'
          }
          placement="top"
        >
          <span className={styles.indexWrap}>
            <IconButton
              size="small"
              className={`${styles.indexBtn}${indexingState === 'queued' ? ` ${styles.indexBtnQueued}` : ''}`}
              disabled={indexingState !== null}
              onClick={(e) => {
                e.preventDefault();
                e.stopPropagation();
                indexModel.mutate();
              }}
            >
              {indexingState === 'running' ? (
                <CircularProgress size={16} className={styles.spinner} />
              ) : (
                <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M17.65 6.35A7.958 7.958 0 0 0 12 4c-4.42 0-7.99 3.58-7.99 8s3.57 8 7.99 8c3.73 0 6.84-2.55 7.73-6h-2.08A5.99 5.99 0 0 1 12 18c-3.31 0-6-2.69-6-6s2.69-6 6-6c1.66 0 3.14.69 4.22 1.78L13 11h7V4l-2.35 2.35z" />
                </svg>
              )}
            </IconButton>
          </span>
        </Tooltip>
      )}
    </AppCard>
  );
}
export default memo(ExplorerModelCard);
