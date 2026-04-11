/**
 * Shared metadata field definitions used by both the folder/directory metadata
 * and per-model metadata override tracks.
 */
export interface CommonMetadataFields {
  creator: string | null;
  collection: string | null;
  subcollection: string | null;
  category: string | null;
  type: string | null;
  material: string | null;
  supported: boolean | null;
  raftHeightMm: number | null;
  modelName?: string | null;
  partName?: string | null;
}
