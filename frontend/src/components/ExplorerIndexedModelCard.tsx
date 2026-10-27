import { memo, useState } from 'react';
import { Pencil } from 'lucide-react';
import { IconButton, Tooltip } from '@jamiehales/ui-kit/mostlymaking';
import type { ExplorerModel, Model } from '../lib/api';
import ModelCard from './ModelCard';
import ModelMetaList from './ModelMetaList';
import ModelMetadataEditor from './ModelMetadataEditor';

interface Props {
  /** The explorer's view of the model, for its metadata and where each value comes from. */
  entry: ExplorerModel;
  model: Model;
}

/** An indexed model in the explorer: its card with the metadata rows and an edit button. */
function ExplorerIndexedModelCard({ entry, model }: Props) {
  const [editorOpen, setEditorOpen] = useState(false);

  return (
    <>
      <ModelCard
        model={model}
        href={`/model/${encodeURIComponent(model.id)}`}
        details={<ModelMetaList model={entry} />}
        extraActions={
          <Tooltip content="Edit metadata">
            <IconButton
              size="sm"
              aria-label={`Edit metadata for ${model.name}`}
              onClick={(e) => {
                e.preventDefault();
                e.stopPropagation();
                setEditorOpen(true);
              }}
            >
              <Pencil size={14} strokeWidth={1.5} />
            </IconButton>
          </Tooltip>
        }
      />
      {/* Mounted only while open: the editor fetches the model's metadata when it mounts. */}
      {editorOpen && (
        <ModelMetadataEditor model={model} open onClose={() => setEditorOpen(false)} />
      )}
    </>
  );
}

export default memo(ExplorerIndexedModelCard);
