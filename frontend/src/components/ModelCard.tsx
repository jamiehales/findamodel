import { memo, useMemo } from 'react';
import { Link, useLocation, useNavigate } from 'react-router-dom';
import { Plus } from 'lucide-react';
import { IconButton } from '@jamiehales/ui-kit/mostlymaking';
import type { Model } from '../lib/api';
import { useActivePrintingList, useUpsertPrintingListItem } from '../lib/queries';
import { appendFilterValue, appendSupportedFilter } from '../lib/modelFilterNavigation';
import { withPreviewSupports } from '../lib/http';
import FilterPill from './FilterPill';
import QuantityStepper from './QuantityStepper';
import { useRenderControls } from './RenderControlsContext';
import styles from './ModelCard.module.css';

function getFileName(relativePath: string): string {
  const parts = relativePath.split('/');
  return parts[parts.length - 1] ?? relativePath;
}

interface ModelCardProps {
  model: Model;
  href: string;
  /** Shown under the name in place of the standard filter tags (user tags stay). */
  details?: React.ReactNode;
  /** Buttons shown before the printing list control. */
  extraActions?: React.ReactNode;
}

/**
 * A model tile. The name is the link and stretches over the whole card, so the
 * filter tags and the printing list buttons can sit on top of it.
 */
function ModelCard({ model, href, details, extraActions }: ModelCardProps) {
  const { showSupports } = useRenderControls();
  const location = useLocation();
  const navigate = useNavigate();
  const { data: activeList } = useActivePrintingList();
  const { mutate: upsert } = useUpsertPrintingListItem();
  const quantity = activeList?.items.find((i) => i.modelId === model.id)?.quantity ?? 0;
  const previewUrl = useMemo(
    () => (model.previewUrl ? withPreviewSupports(model.previewUrl, showSupports) : null),
    [model.previewUrl, showSupports],
  );

  function filterBy(e: React.MouseEvent, nextSearch: (source: string) => string) {
    e.preventDefault();
    e.stopPropagation();
    const sourceSearch = location.pathname === '/' ? location.search : '';
    const search = nextSearch(sourceSearch);
    navigate({ pathname: '/', search: search ? `?${search}` : '' });
  }

  function setQuantity(next: number) {
    if (!activeList) return;
    upsert({ listId: activeList.id, modelId: model.id, quantity: Math.max(0, next) });
  }

  const standard: { key: 'fileType' | 'material' | 'category' | 'type'; value: string | null }[] =
    details
      ? []
      : [
          { key: 'fileType', value: model.fileType },
          { key: 'material', value: model.material },
          { key: 'category', value: model.category },
          { key: 'type', value: model.type },
        ];

  return (
    <div className={`mm-card-link ${styles.card}`} data-detailed={details ? '' : undefined}>
      <div className={`mm-card-image ${styles.image}`}>
        {previewUrl ? (
          <img src={previewUrl} alt="" loading="lazy" />
        ) : (
          <span className={styles.placeholder} aria-hidden="true">
            {model.fileType.toUpperCase()}
          </span>
        )}
      </div>
      <div className={styles.body}>
        <Link to={href} className={styles.name}>
          {model.name}
        </Link>
        <span className={styles.file}>{getFileName(model.relativePath)}</span>
        {!(details && (model.tags ?? []).length === 0) && (
          <div className={styles.tags}>
            {standard.map(({ key, value }) =>
              value ? (
                <FilterPill
                  key={key}
                  label={key === 'fileType' ? value.toUpperCase() : value}
                  onClick={(e) => filterBy(e, (s) => appendFilterValue(s, key, value))}
                />
              ) : null,
            )}
            {!details && model.supported !== null && (
              <FilterPill
                label={model.supported ? 'Supported' : 'Unsupported'}
                tone={model.supported ? 'supported' : 'unsupported'}
                onClick={(e) => filterBy(e, (s) => appendSupportedFilter(s, model.supported!))}
              />
            )}
            {(model.tags ?? []).map((tag) => (
              <FilterPill
                key={`user-${tag}`}
                label={tag}
                tone="user"
                onClick={(e) => filterBy(e, (s) => appendFilterValue(s, 'tags', tag))}
              />
            ))}
          </div>
        )}
        {details}
      </div>
      <div className={styles.actions} data-pinned={quantity > 0 ? '' : undefined}>
        {extraActions}
        {quantity > 0 ? (
          <QuantityStepper value={quantity} onChange={setQuantity} label={model.name} size="sm" />
        ) : (
          <IconButton
            size="sm"
            aria-label={`Add ${model.name} to printing list`}
            disabled={!activeList}
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setQuantity(1);
            }}
          >
            <Plus size={16} strokeWidth={1.5} />
          </IconButton>
        )}
      </div>
    </div>
  );
}

export default memo(ModelCard);
