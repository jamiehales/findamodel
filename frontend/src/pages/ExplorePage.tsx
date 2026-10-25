import { useState } from 'react';
import { useParams } from 'react-router-dom';
import { RefreshCw } from 'lucide-react';
import { Button, IconButton, Spinner, Tooltip } from '@jamiehales/ui-kit/mostlymaking';
import { useExplorer, useIndexFolder, useIsFolderIndexing } from '../lib/queries';
import type { ExplorerModel, Model } from '../lib/api';
import { explorerFileUrl } from '../lib/api';
import ErrorView from '../components/ErrorView';
import FolderCard from '../components/FolderCard';
import ExplorerFileRow from '../components/ExplorerFileCard';
import ExplorerModelCard from '../components/ExplorerModelCard';
import ModelCard from '../components/ModelCard';
import LoadingView from '../components/LoadingView';
import MetadataEditor from '../components/MetadataEditor';
import PathBreadcrumb from '../components/PathBreadcrumb';
import PageLayout, { PageHeader } from '../components/layouts/PageLayout';
import CardGrid from '../components/CardGrid';
import explorerStyles from '../components/Explorer.module.css';
import statusStyles from '../components/StatusView.module.css';
import styles from './ExplorePage.module.css';

function toProcessedModel(model: ExplorerModel): Model | null {
  if (!model.id) return null;

  const nameFromFile = model.fileName.replace(/\.[^.]+$/, '');

  return {
    id: model.id,
    name: model.resolvedMetadata?.modelName ?? nameFromFile,
    partName: model.resolvedMetadata?.partName ?? null,
    relativePath: model.relativePath,
    fileType: model.fileType,
    canExportToPlate:
      model.fileType.toLowerCase() === 'stl' || model.fileType.toLowerCase() === 'obj',
    fileSize: model.fileSize ?? 0,
    fileUrl: explorerFileUrl(model.relativePath),
    hasPreview: model.hasPreview,
    previewUrl: model.previewUrl,
    creator: model.resolvedMetadata?.creator ?? null,
    collection: model.resolvedMetadata?.collection ?? null,
    subcollection: model.resolvedMetadata?.subcollection ?? null,
    tags: model.resolvedMetadata?.tags ?? [],
    category: model.resolvedMetadata?.category ?? null,
    type: model.resolvedMetadata?.type ?? null,
    material: model.resolvedMetadata?.material ?? null,
    supported: model.resolvedMetadata?.supported ?? null,
    supportSplit: null,
    convexHull: null,
    concaveHull: null,
    convexSansRaftHull: null,
    raftHeightMm: model.resolvedMetadata?.raftHeightMm ?? 0,
    dimensionXMm: null,
    dimensionYMm: null,
    dimensionZMm: null,
    sphereCentreX: null,
    sphereCentreY: null,
    sphereCentreZ: null,
    sphereRadius: null,
  };
}

function Section({
  title,
  count,
  children,
}: {
  title: string;
  count: number;
  children: React.ReactNode;
}) {
  const id = `section-${title.toLowerCase()}`;
  return (
    <section className={styles.section} aria-labelledby={id}>
      <div className={styles.sectionHeader}>
        <h2 id={id} className="mm-type-label">
          {title}
        </h2>
        <span className={styles.badge}>{count}</span>
      </div>
      {children}
    </section>
  );
}

function ExplorePageInner({ path }: { path: string }) {
  const { data, isPending, isError } = useExplorer(path);

  if (isPending) return <LoadingView />;
  if (isError) return <ErrorView message="Couldn't open this folder. Check that it exists." />;

  const isEmpty = data.folders.length === 0 && data.models.length === 0 && data.files.length === 0;
  if (isEmpty) return <p className={statusStyles.empty}>This folder is empty.</p>;

  return (
    <>
      {data.folders.length > 0 && (
        <Section title="Folders" count={data.folders.length}>
          <div className={styles.folders}>
            {data.folders.map((folder) => (
              <FolderCard key={folder.path} folder={folder} href={`/explore/${folder.path}`} />
            ))}
          </div>
        </Section>
      )}

      {data.models.length > 0 && (
        <Section title="Models" count={data.models.length}>
          <CardGrid minCardWidth={180}>
            {data.models.map((model) => {
              const processed = toProcessedModel(model);
              return processed ? (
                <ModelCard
                  key={processed.id}
                  model={processed}
                  href={`/model/${encodeURIComponent(processed.id)}`}
                />
              ) : (
                <ExplorerModelCard key={model.relativePath} model={model} />
              );
            })}
          </CardGrid>
        </Section>
      )}

      {data.files.length > 0 && (
        <Section title="Files" count={data.files.length}>
          <div className={explorerStyles.files}>
            {data.files.map((file) => (
              <ExplorerFileRow key={file.relativePath} file={file} />
            ))}
          </div>
        </Section>
      )}
    </>
  );
}

export default function ExplorePage() {
  const params = useParams();
  const path = params['*'] ?? '';
  const [metadataOpen, setMetadataOpen] = useState(false);
  const indexFolder = useIndexFolder(path);
  const indexingState = useIsFolderIndexing(path);
  const segments = path === '' ? [] : path.split('/');
  const title = segments[segments.length - 1] ?? 'Explore';

  return (
    <PageLayout>
      <PageHeader
        before={segments.length > 0 ? <PathBreadcrumb path={path} /> : undefined}
        title={title}
        actions={
          <>
            <Button variant="secondary" onClick={() => setMetadataOpen(true)}>
              Edit metadata
            </Button>
            <Tooltip
              content={
                indexingState === 'running'
                  ? 'Indexing'
                  : indexingState === 'queued'
                    ? 'Queued'
                    : 'Index folder'
              }
            >
              <IconButton
                aria-label="Index folder"
                onClick={() => indexFolder.mutate()}
                disabled={indexingState !== null}
              >
                {indexingState === 'running' ? (
                  <Spinner label="Indexing" />
                ) : (
                  <RefreshCw size={18} strokeWidth={1.5} />
                )}
              </IconButton>
            </Tooltip>
          </>
        }
      />

      <ExplorePageInner path={path} />

      <MetadataEditor path={path} open={metadataOpen} onClose={() => setMetadataOpen(false)} />
    </PageLayout>
  );
}
