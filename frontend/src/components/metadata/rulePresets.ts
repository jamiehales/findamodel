/**
 * Ready-made regex rules for the folder metadata editor. Rules match against the model's
 * path relative to the library root with a leading slash, e.g.
 * "/Creator/Collection/Model/part.stl", so folder levels count from the library root.
 */

export interface RulePreset {
  id: string;
  label: string;
  /** Extra context shown beside the label, e.g. the folder name at that level. */
  detail?: string;
  yaml: string;
}

export interface RulePresetGroup {
  label: string;
  presets: RulePreset[];
}

/** Levels offered beyond the folder being edited. */
const EXTRA_LEVELS = 3;
const MAX_LEVELS = 8;

const TEXT_FIELDS = new Set(['model_name', 'part_name', 'creator', 'collection', 'subcollection']);
const SELECT_FIELDS = new Set(['category', 'type', 'material']);

/** Single-quoted YAML scalar, so regex characters never need escaping. */
function quoted(value: string): string {
  return `'${value.replace(/'/g, "''")}'`;
}

function expressionYaml(expression: string): string {
  return `expression: ${quoted(expression)}`;
}

function valuesYaml(entries: [string, string][]): string {
  return [
    'values:',
    ...entries.map(([key, pattern]) => `  ${quoted(key)}: ${quoted(pattern)}`),
  ].join('\n');
}

function ordinal(n: number): string {
  const tens = n % 100;
  if (tens >= 11 && tens <= 13) return `${n}th`;
  return `${n}${['th', 'st', 'nd', 'rd'][n % 10] ?? 'th'}`;
}

/** Matches `pattern` only when no letter or digit touches it, so "_" and "-" separate
 *  words ("elf_32mm_ps" contains "32mm") where `\b` would not. */
function standalone(pattern: string): string {
  return `(?<![a-z0-9])${pattern}(?![a-z0-9])`;
}

function escapeRegex(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** Regex capturing the folder `level` levels below the library root (1 = top level). */
export function folderLevelExpression(level: number): string {
  return `^/${'[^/]+/'.repeat(level - 1)}([^/]+)/`;
}

const LEVEL_CAPTURE = '([^/]+)/';

/** Ways to tidy a captured folder name, applied to a rule that captures a whole folder. */
export const RULE_TRIMS = [
  {
    id: 'brackets',
    label: 'Drop text in brackets',
    capture: '([^/]+?)(?: \\([^/]*\\))?/',
  },
  {
    id: 'dash',
    label: 'Drop text after " - "',
    capture: '([^/]+?)(?: - [^/]*)?/',
  },
] as const;

/** The rule text with its whole-folder capture swapped for a trimmed one, or null when the
 *  rule has no whole-folder capture to trim. */
export function applyTrim(
  ruleText: string,
  trimId: (typeof RULE_TRIMS)[number]['id'],
): string | null {
  const trim = RULE_TRIMS.find((t) => t.id === trimId);
  if (!trim || !ruleText.includes(LEVEL_CAPTURE)) return null;
  return ruleText.replace(LEVEL_CAPTURE, trim.capture);
}

/** Letter cases a text rule's result can be re-cased to (the rule's `case` option). */
export const RULE_CASES = [
  { id: 'none', label: 'As written' },
  { id: 'title', label: 'Title case' },
  { id: 'sentence', label: 'Sentence case' },
  { id: 'upper', label: 'Upper case' },
  { id: 'lower', label: 'Lower case' },
] as const;

export type RuleCase = (typeof RULE_CASES)[number]['id'];

const CASE_LINE = /^case:.*(?:\n|$)/im;

/** Whether a field's rule results are text that can be re-cased. */
export function supportsCase(field: string): boolean {
  return TEXT_FIELDS.has(field) || field === 'tags';
}

/** The `case` a rule sets, or 'none'. */
export function ruleCase(ruleText: string): RuleCase {
  const value = ruleText.match(/^case:\s*'?"?([a-z]+)/im)?.[1]?.toLowerCase();
  return RULE_CASES.find((c) => c.id === value)?.id ?? 'none';
}

/** The rule text with its `case` option set (or removed for 'none'). */
export function withRuleCase(ruleText: string, caseId: RuleCase): string {
  const without = ruleText.replace(CASE_LINE, '').trimEnd();
  return caseId === 'none' ? without : `${without}\ncase: ${caseId}`;
}

export function rulePresetGroups(
  field: string,
  folderPath: string,
  options: { selectValues?: string[]; tagValues?: string[] } = {},
): RulePresetGroup[] {
  const segments = folderPath.split('/').filter(Boolean);
  const groups: RulePresetGroup[] = [];

  if (TEXT_FIELDS.has(field)) {
    const levels = Math.min(Math.max(segments.length + EXTRA_LEVELS, 3), MAX_LEVELS);
    groups.push({
      label: 'Folder from library root',
      presets: Array.from({ length: levels }, (_, i) => ({
        id: `level-${i + 1}`,
        label: `${ordinal(i + 1)} folder`,
        detail: segments[i],
        yaml: expressionYaml(folderLevelExpression(i + 1)),
      })),
    });
    groups.push({
      label: 'From the file',
      presets: [
        {
          id: 'parent-folder',
          label: 'Folder holding the file',
          yaml: expressionYaml('/([^/]+)/[^/]+$'),
        },
        {
          id: 'file-name',
          label: 'File name',
          detail: 'without extension',
          yaml: expressionYaml('/([^/]+)\\.[^./]+$'),
        },
      ],
    });
  }

  if (field === 'supported') {
    groups.push({
      label: 'Supported',
      presets: [
        {
          id: 'supported-keywords',
          label: 'Supported keywords',
          detail: 'and unsupported',
          // Unsupported first: the first matching key wins, and "unsupported" contains
          // "supported". Paths matching neither stay unknown.
          yaml: valuesYaml([
            ['Unsupported', 'un-?supported|(?<![a-z])no.?supp'],
            ['Supported', 'supported|_ps(?![a-z0-9])'],
          ]),
        },
        {
          id: 'supported-folder',
          label: 'Folder named Supported',
          yaml: expressionYaml('/supported/'),
        },
      ],
    });
  }

  if (field === 'tags') {
    const presets: RulePreset[] = [
      {
        id: 'scale',
        label: 'Scale from path',
        detail: 'e.g. 32mm',
        yaml: expressionYaml(standalone('(\\d+mm)')),
      },
    ];
    const tagValues = options.tagValues ?? [];
    if (tagValues.length > 0) {
      presets.push({
        id: 'tag-keywords',
        label: 'Tags named in path',
        detail: `${tagValues.length} tags`,
        yaml: valuesYaml(tagValues.map((tag) => [tag, standalone(escapeRegex(tag))])),
      });
    }
    groups.push({ label: 'Tags', presets });
  }

  if (SELECT_FIELDS.has(field) && (options.selectValues?.length ?? 0) > 0) {
    const values = options.selectValues ?? [];
    groups.push({
      label: 'Values',
      presets: [
        {
          id: 'value-keywords',
          label: 'Values named in path',
          detail: `${values.length} values`,
          yaml: valuesYaml(values.map((value) => [value, standalone(escapeRegex(value))])),
        },
      ],
    });
  }

  return groups;
}
