/**
 * FRIDAY result presentation — pure classification and formatting for a goal's
 * execution result (`goal.completed.result`).
 *
 * The runtime result arrives as a `GoalResult`: mostly a `kind` plus a `text`
 * string (and sometimes an explicit `structured` payload). The catch is that a
 * "text" result is *often JSON text* — the filesystem adapter serializes its
 * directory listing as a JSON string, and other tools do the same. Rendering
 * that string as-is is the old raw-JSON-blob card. This module classifies the
 * result into a typed, renderable view (`ResultView`) and formats each shape
 * into human-readable pieces.
 *
 * Pure on purpose: no DOM, no React — so the classification/formatting is unit
 * testable with `node --test` against the exact schema the daemon emits.
 */

import type { GoalResult } from '../protocol/protocol';

/** One directory entry as emitted by the filesystem adapter. */
export interface FileEntry {
  name: string;
  is_dir?: boolean;
  isDir?: boolean;
  [key: string]: unknown;
}

/** The filesystem adapter's listing payload: `{ path, entries }`. */
export interface FileListing {
  path?: string;
  entries: FileEntry[];
}

/** A classified result — the discriminated view the card renders. */
export type ResultView =
  | { type: 'empty' }
  | { type: 'error'; text: string }
  | { type: 'text'; text: string }
  | { type: 'fileList'; path?: string; entries: FileEntry[] }
  | { type: 'array'; items: unknown[] }
  | { type: 'structured'; data: Record<string, unknown> }
  | { type: 'raw'; data: unknown };

/** Parse a string as JSON, returning `null` when it is not JSON. */
export function tryParseJson(text: string): unknown | null {
  const trimmed = text.trim();
  if (!trimmed) return null;
  try {
    return JSON.parse(trimmed);
  } catch {
    return null;
  }
}

/** True when a value looks like the filesystem adapter's listing payload. */
export function isFileListing(value: unknown): value is FileListing {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const o = value as Record<string, unknown>;
  if (!Array.isArray(o.entries)) return false;
  return o.entries.every((e) => {
    if (!e || typeof e !== 'object' || Array.isArray(e)) return false;
    return typeof (e as Record<string, unknown>).name === 'string';
  });
}

/**
 * Classify a `goal.completed.result` into a renderable view.
 *
 * Order matters:
 * 1. absent / empty / error results map straight to their view;
 * 2. a `structured` kind uses its explicit payload when present;
 * 3. a `text` kind is inspected — JSON text (the filesystem adapter's listing,
 *    an array, a generic object) reveals its true shape;
 * 4. plain text stays plain text.
 */
export function classifyResult(result: GoalResult | undefined): ResultView {
  if (!result) return { type: 'empty' };
  if (result.kind === 'empty') return { type: 'empty' };
  if (result.kind === 'error') {
    return { type: 'error', text: result.text.trim() || 'The goal could not be completed.' };
  }

  const text = result.text ?? '';
  const parsed = text.trim() ? tryParseJson(text) : null;

  if (result.kind === 'structured' && result.structured !== undefined) {
    const s = result.structured;
    if (isFileListing(s)) return { type: 'fileList', path: s.path, entries: s.entries };
    if (Array.isArray(s)) return { type: 'array', items: s };
    if (s && typeof s === 'object') return { type: 'structured', data: s as Record<string, unknown> };
    return { type: 'raw', data: s };
  }

  if (parsed !== null) {
    if (isFileListing(parsed)) return { type: 'fileList', path: parsed.path, entries: parsed.entries };
    if (Array.isArray(parsed)) return { type: 'array', items: parsed };
    if (typeof parsed === 'object') return { type: 'structured', data: parsed as Record<string, unknown> };
    return { type: 'text', text: String(parsed) };
  }

  if (text.trim()) return { type: 'text', text };
  return { type: 'empty' };
}

/** True for a directory entry (accepts the adapter's `is_dir` and the camelCase form). */
export function isDirEntry(entry: FileEntry): boolean {
  return entry.is_dir === true || entry.isDir === true;
}

/** The icon key for each entry, derived from its directory flag. */
export function fileListLines(entries: FileEntry[]): Array<{ name: string; icon: 'file' | 'folder' }> {
  return entries.map((e) => ({ name: e.name, icon: isDirEntry(e) ? 'folder' : 'file' }));
}

/** `1 entry` / `12 entries` — the summary count label. */
export function entryCountLabel(n: number): string {
  return n === 1 ? '1 entry' : `${n} entries`;
}

/** Human-readable rows for a structured object: scalar values inline, nested
 * values summarized (with the raw payload reachable via "Execution details"). */
export function structureRows(data: Record<string, unknown>): Array<{ key: string; value: string }> {
  return Object.entries(data).map(([key, value]) => ({ key, value: summarizeValue(value) }));
}

function summarizeValue(value: unknown): string {
  if (value === null) return 'null';
  if (Array.isArray(value)) {
    if (value.length === 0) return '[]';
    const allScalar = value.every((v) => v === null || typeof v !== 'object');
    return allScalar ? value.map((v) => String(v)).join(', ') : `${value.length} item${value.length === 1 ? '' : 's'}`;
  }
  if (typeof value === 'object') {
    const keys = Object.keys(value as Record<string, unknown>);
    if (keys.length === 0) return '{}';
    return `${keys.length} field${keys.length === 1 ? '' : 's'}`;
  }
  return String(value);
}

/** One readable line per array item; objects collapse to their scalar fields. */
export function arrayLines(items: unknown[]): string[] {
  return items.map((item) => {
    if (item === null) return 'null';
    if (typeof item === 'object') {
      const o = item as Record<string, unknown>;
      const scalars = Object.entries(o)
        .filter(([, v]) => v === null || typeof v !== 'object')
        .map(([k, v]) => `${k}: ${v}`);
      return scalars.length ? scalars.join(' · ') : JSON.stringify(item);
    }
    return String(item);
  });
}
