import Ajv2020 from 'ajv/dist/2020';
import type { ErrorObject } from 'ajv';
import schema from '../../../schema/export.v1.json';
import type { LineageExport } from './types';

export const SUPPORTED_VERSION = 1;

export type ParseResult =
  | { ok: true; data: LineageExport }
  | { ok: false; title: string; message: string; details: string[] };

const ajv = new Ajv2020({ allowUnionTypes: true });
const validateSchema = ajv.compile(schema);

function fail(title: string, message: string, details: string[] = []): ParseResult {
  return { ok: false, title, message, details };
}

/// "/nodes/3/hash" becomes "nodes[3].hash".
export function prettyPath(instancePath: string): string {
  if (!instancePath) return 'the file';
  return instancePath
    .split('/')
    .slice(1)
    .map((p) => p.replace(/~1/g, '/').replace(/~0/g, '~'))
    .reduce((acc, part) => (/^\d+$/.test(part) ? `${acc}[${part}]` : acc ? `${acc}.${part}` : part), '');
}

export function describeError(e: ErrorObject): string {
  const where = prettyPath(e.instancePath);
  switch (e.keyword) {
    case 'required':
      return `${where} is missing the field "${(e.params as { missingProperty: string }).missingProperty}"`;
    case 'additionalProperties':
      return `${where} has an unexpected field "${(e.params as { additionalProperty: string }).additionalProperty}"`;
    case 'type':
      return `${where} should be of type ${(e.params as { type: string | string[] }).type}`;
    case 'const':
      return `${where} should be ${JSON.stringify((e.params as { allowedValue: unknown }).allowedValue)}`;
    case 'enum':
      return `${where} should be one of ${((e.params as { allowedValues: unknown[] }).allowedValues).map((v) => JSON.stringify(v)).join(', ')}`;
    default:
      return `${where} ${e.message ?? 'is not valid'}`;
  }
}

/// Cross checks the schema cannot express: every reference points at a real node.
export function checkReferences(d: LineageExport): string[] {
  const problems: string[] = [];
  const hashes = new Set<string>();
  for (const n of d.nodes) {
    if (hashes.has(n.hash)) problems.push(`version ${n.short} appears twice`);
    hashes.add(n.hash);
  }
  if (!hashes.has(d.root)) problems.push('the root version is not in nodes');
  if (!hashes.has(d.current)) problems.push('the current version is not in nodes');
  d.edges.forEach((e, i) => {
    if (!hashes.has(e.parent)) problems.push(`edges[${i}] points at a parent that is not in nodes`);
    if (!hashes.has(e.child)) problems.push(`edges[${i}] points at a child that is not in nodes`);
  });
  d.nodes.forEach((n, i) => {
    if (n.parent !== null && !hashes.has(n.parent)) problems.push(`nodes[${i}] has a parent that is not in nodes`);
  });
  d.refused.forEach((r, i) => {
    if (!hashes.has(r.parent)) problems.push(`refused[${i}] points at a parent that is not in nodes`);
  });
  d.series.forEach((p, i) => {
    if (!hashes.has(p.current_after)) problems.push(`series[${i}] points at a version that is not in nodes`);
  });
  return problems;
}

/// Turn the text of a file into a checked export, or a friendly error.
export function parseExport(text: string): ParseResult {
  if (text.trim() === '') {
    return fail('That file is empty', 'Choose a tree.json written by `bogo export --out tree.json`.');
  }
  let doc: unknown;
  try {
    doc = JSON.parse(text);
  } catch (err) {
    return fail(
      'That file is not valid JSON',
      'The viewer reads the JSON file that `bogo export --out tree.json` writes. The file may be cut off or may be a different kind of file.',
      [err instanceof Error ? err.message : String(err)],
    );
  }
  if (typeof doc !== 'object' || doc === null || Array.isArray(doc)) {
    return fail('That is not a lineage export', 'The top level of the file should be a JSON object.');
  }
  const head = doc as { format?: unknown; version?: unknown };
  // rules-evolve-export: files written by rules-evolve 2.x, before the rename.
  if (head.format !== 'bogovolution-export' && head.format !== 'rules-evolve-export') {
    return fail(
      'That does not look like a bogo export',
      'The file has no "format": "bogovolution-export" marker. Make one with `bogo export --out tree.json`.',
    );
  }
  if (head.version !== SUPPORTED_VERSION) {
    const newer = typeof head.version === 'number' && head.version > SUPPORTED_VERSION;
    return fail(
      newer ? 'That export is newer than this viewer' : 'Unsupported export version',
      newer
        ? `This viewer reads export version ${SUPPORTED_VERSION}, and the file is version ${String(head.version)}. Build a newer viewer from the same repo.`
        : `This viewer reads export version ${SUPPORTED_VERSION}, and the file says ${JSON.stringify(head.version)}.`,
    );
  }
  if (!validateSchema(doc)) {
    const errors = validateSchema.errors ?? [];
    return fail(
      'That export does not match the schema',
      'The file looks like a lineage export, but some fields are missing or have the wrong shape. It may have been edited by hand.',
      errors.slice(0, 5).map(describeError),
    );
  }
  const data = doc as unknown as LineageExport;
  const problems = checkReferences(data);
  if (problems.length > 0) {
    return fail(
      'That export is inconsistent',
      'The file has the right shape, but some parts refer to versions that are not in it.',
      problems.slice(0, 6),
    );
  }
  return { ok: true, data };
}
