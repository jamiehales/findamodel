import React from 'react';
import { Link, useParams } from 'react-router-dom';
import { ChevronLeft, Download, RefreshCw } from 'lucide-react';
import {
  Button,
  IconButton,
  Spinner,
  Tag,
  TextLink,
  Tooltip,
} from '@jamiehales/ui-kit/mostlymaking';
import {
  useModel,
  useModelOtherParts,
  useActivePrintingList,
  useUpsertPrintingListItem,
  useIndexModel,
  useIsModelIndexing,
} from '../lib/queries';
import ModelViewer from '../components/ModelViewer';
import ErrorView from '../components/ErrorView';
import HullPreview from '../components/HullPreview';
import LoadingView from '../components/LoadingView';
import PathBreadcrumb from '../components/PathBreadcrumb';
import ModelCard from '../components/ModelCard';
import CardGrid from '../components/CardGrid';
import PageLayout from '../components/layouts/PageLayout';
import ModelMetadataEditor from '../components/ModelMetadataEditor';
import QuantityStepper from '../components/QuantityStepper';
import styles from './ModelPage.module.css';
import { formatBytes } from '../lib/utils';

function filterHref(key: string, value: string): string {
  return `/?${new URLSearchParams({ [key]: value }).toString()}`;
}

function BackLink() {
  return (
    <Button asChild variant="ghost" size="sm" className={styles.back}>
      <Link to="/">
        <ChevronLeft size={16} strokeWidth={1.5} aria-hidden="true" />
        Models
      </Link>
    </Button>
  );
}

function ModelPage() {
  const { id } = useParams<{ id: string }>();
  const decodedId = decodeURIComponent(id ?? '');
  const [metadataOpen, setMetadataOpen] = React.useState(false);

  React.useEffect(() => {
    window.scrollTo({ top: 0, left: 0, behavior: 'auto' });
  }, [decodedId]);

  const { data: model, isPending, isError } = useModel(decodedId);
  const { data: otherParts } = useModelOtherParts(decodedId);
  const { data: activeList } = useActivePrintingList();
  const { mutate: upsertItem } = useUpsertPrintingListItem();
  const qty = model ? (activeList?.items.find((i) => i.modelId === model.id)?.quantity ?? 0) : 0;

  const { mutate: indexModel } = useIndexModel(model?.relativePath ?? '');
  const indexingStatus = useIsModelIndexing(model?.relativePath ?? '');

  if (isPending) {
    return (
      <PageLayout>
        <BackLink />
        <LoadingView minHeight="60vh" />
      </PageLayout>
    );
  }

  if (isError || model === null) {
    return (
      <PageLayout>
        <BackLink />
        <ErrorView message="Model not found." minHeight="60vh" />
      </PageLayout>
    );
  }

  const setQuantity = (next: number) => {
    if (!activeList) return;
    upsertItem({ listId: activeList.id, modelId: model.id, quantity: Math.max(0, next) });
  };

  const dimensions =
    model.dimensionXMm != null && model.dimensionYMm != null && model.dimensionZMm != null
      ? `${model.dimensionXMm.toFixed(1)} × ${model.dimensionYMm.toFixed(1)} × ${model.dimensionZMm.toFixed(1)} mm`
      : null;

  const metaRows: { label: string; value: React.ReactNode; mono?: boolean }[] = [
    model.partName && { label: 'Part', value: model.partName },
    model.creator && {
      label: 'Creator',
      value: (
        <TextLink asChild>
          <Link to={filterHref('creator', model.creator)}>{model.creator}</Link>
        </TextLink>
      ),
    },
    model.collection && {
      label: 'Collection',
      value: (
        <TextLink asChild>
          <Link to={filterHref('collection', model.collection)}>{model.collection}</Link>
        </TextLink>
      ),
    },
    model.subcollection && {
      label: 'Subcollection',
      value: (
        <TextLink asChild>
          <Link to={filterHref('subcollection', model.subcollection)}>{model.subcollection}</Link>
        </TextLink>
      ),
    },
    model.category && { label: 'Category', value: model.category },
    model.type && { label: 'Type', value: model.type },
    model.material && { label: 'Material', value: model.material },
    dimensions && { label: 'Size', value: dimensions, mono: true },
    { label: 'Raft height', value: `${model.raftHeightMm} mm`, mono: true },
  ].filter(Boolean) as { label: string; value: React.ReactNode; mono?: boolean }[];

  const hasHulls = !!(model.convexHull || model.concaveHull || model.convexSansRaftHull);

  return (
    <PageLayout>
      <BackLink />

      <div className={styles.top}>
        <section className={styles.stage} aria-label="3D preview">
          <ModelViewer
            modelId={model.id}
            convexHull={model.convexHull}
            concaveHull={model.concaveHull}
            convexSansRaftHull={model.convexSansRaftHull}
            supported={model.supported}
          />
        </section>

        <section className={styles.panel} aria-label="Model details">
          <div className={styles.titleGroup}>
            <h1 className="mm-type-title-lg">{model.name}</h1>
            <PathBreadcrumb path={model.relativePath} />
            <span className={styles.fileMeta}>{formatBytes(model.fileSize)}</span>
          </div>

          <div className={styles.tags}>
            <Tag variant="neutral">{model.fileType.toUpperCase()}</Tag>
            {model.supported != null && (
              <Tag variant={model.supported ? 'success' : 'outline'}>
                {model.supported ? 'Supported' : 'Unsupported'}
              </Tag>
            )}
            {model.tags.map((tag) => (
              <Tag key={tag} asChild>
                <Link to={filterHref('tags', tag)}>{tag}</Link>
              </Tag>
            ))}
          </div>

          <div className={styles.actions}>
            <QuantityStepper value={qty} onChange={setQuantity} label={model.name} />
            <Button variant="primary" disabled={!activeList} onClick={() => setQuantity(qty + 1)}>
              {qty > 0 ? 'Add another' : 'Add to list'}
            </Button>
            <div className={styles.spacer} />
            <Button variant="secondary" onClick={() => setMetadataOpen(true)}>
              Edit
            </Button>
            <Tooltip content={`Download .${model.fileType}`}>
              <IconButton asChild aria-label={`Download .${model.fileType}`}>
                <a href={model.fileUrl} download={`${model.name}.${model.fileType}`}>
                  <Download size={18} strokeWidth={1.5} />
                </a>
              </IconButton>
            </Tooltip>
            <Tooltip
              content={
                indexingStatus === 'running'
                  ? 'Reindexing'
                  : indexingStatus === 'queued'
                    ? 'Reindex queued'
                    : 'Reindex'
              }
            >
              <IconButton
                aria-label="Reindex model"
                onClick={() => indexModel()}
                disabled={indexingStatus !== null}
              >
                {indexingStatus === 'running' ? (
                  <Spinner label="Reindexing" />
                ) : (
                  <RefreshCw size={18} strokeWidth={1.5} />
                )}
              </IconButton>
            </Tooltip>
          </div>

          {metaRows.length > 0 && (
            <dl className={styles.kv}>
              {metaRows.map(({ label, value, mono }) => (
                <React.Fragment key={label}>
                  <dt>{label}</dt>
                  <dd className={mono ? styles.mono : undefined}>{value}</dd>
                </React.Fragment>
              ))}
            </dl>
          )}
        </section>
      </div>

      {hasHulls && (
        <section className={styles.section} aria-labelledby="hulls-heading">
          <h2 id="hulls-heading" className="mm-type-label">
            Hull projections
          </h2>
          <HullPreview
            convexHull={model.convexHull}
            concaveHull={model.concaveHull}
            convexSansRaftHull={model.convexSansRaftHull}
            label="Hull projections"
          />
        </section>
      )}

      {(otherParts?.length ?? 0) > 0 && (
        <section className={styles.section} aria-labelledby="parts-heading">
          <h2 id="parts-heading" className="mm-type-label">
            Other parts
          </h2>
          <CardGrid minCardWidth={170}>
            {otherParts!.map((part) => (
              <ModelCard
                key={part.id}
                href={`/model/${encodeURIComponent(part.id)}`}
                model={{
                  ...model,
                  id: part.id,
                  name: part.name,
                  relativePath: part.relativePath,
                  fileType: part.fileType,
                  canExportToPlate: part.fileType === 'stl' || part.fileType === 'obj',
                  fileSize: part.fileSize,
                  fileUrl: '',
                  hasPreview: part.previewUrl != null,
                  previewUrl: part.previewUrl,
                }}
              />
            ))}
          </CardGrid>
        </section>
      )}

      <ModelMetadataEditor
        model={model}
        open={metadataOpen}
        onClose={() => setMetadataOpen(false)}
      />
    </PageLayout>
  );
}

export default ModelPage;
