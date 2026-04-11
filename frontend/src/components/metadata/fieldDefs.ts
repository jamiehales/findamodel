export type FieldType = 'text' | 'select' | 'bool' | 'number';

export interface SharedFieldDef {
  /** camelCase key matching the API DTO and FormState */
  key: string;
  /**
   * YAML key used for folder rule mode. Only set for fields that support rules
   * (omit for model-only or rule-unsupported fields).
   */
  yamlName?: string;
  label: string;
  fieldType: FieldType;
  /** Which dictionary bucket drives the select options */
  optionsField?: 'category' | 'type' | 'material';
  /** Whether the field can be set as a rule in the folder editor. Defaults to true when yamlName is set. */
  supportsRules?: boolean;
}

/**
 * Fields shared between both the folder metadata editor and the model metadata editor.
 * Ordered for display across the app.
 */
export const SHARED_FIELDS: SharedFieldDef[] = [
  { key: 'modelName', yamlName: 'model_name', label: 'Model Name', fieldType: 'text' },
  { key: 'partName', yamlName: 'part_name', label: 'Part Name', fieldType: 'text' },
  { key: 'creator', yamlName: 'creator', label: 'Creator', fieldType: 'text' },
  { key: 'collection', yamlName: 'collection', label: 'Collection', fieldType: 'text' },
  { key: 'subcollection', yamlName: 'subcollection', label: 'Subcollection', fieldType: 'text' },
  {
    key: 'category',
    yamlName: 'category',
    label: 'Category',
    fieldType: 'select',
    optionsField: 'category',
  },
  { key: 'type', yamlName: 'type', label: 'Type', fieldType: 'select', optionsField: 'type' },
  {
    key: 'material',
    yamlName: 'material',
    label: 'Material',
    fieldType: 'select',
    optionsField: 'material',
  },
  { key: 'supported', yamlName: 'supported', label: 'Supported', fieldType: 'bool' },
  {
    key: 'raftHeightMm',
    yamlName: 'raftHeight',
    label: 'Raft Height (mm)',
    fieldType: 'number',
    supportsRules: false,
  },
];
