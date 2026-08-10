/**
 * FRIDAY result presentation — unit tests.
 *
 * Run: `npm test` (Node 24 native TS type-stripping + `node --test`; no
 * framework needed). The fixtures mirror the exact `goal.completed.result`
 * schema the daemon emits — including the filesystem adapter's listing, which
 * arrives as a JSON *string* inside a `text` result.
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';

import type { GoalResult } from '../src/protocol/protocol';
import {
  arrayLines,
  classifyResult,
  entryCountLabel,
  fileListLines,
  isDirEntry,
  isFileListing,
  structureRows,
  tryParseJson,
} from '../src/screens/resultPresentation.ts';

const textResult = (text: string, kind: GoalResult['kind'] = 'text'): GoalResult => ({
  kind,
  text,
  success: true,
});

/** The exact filesystem-listing result the daemon emits today. */
const filesystemListing: GoalResult = {
  kind: 'text',
  text: JSON.stringify(
    {
      path: '.',
      entries: [
        { name: '.anchored-summary.md', is_dir: false },
        { name: '.env', is_dir: false },
        { name: '.git', is_dir: true },
        { name: 'architecture', is_dir: true },
        { name: 'frontend', is_dir: true },
      ],
    },
    null,
    2,
  ),
  tool: 'filesystem',
  success: true,
};

test('plain text results stay plain text', () => {
  const view = classifyResult(textResult('The FRIDAY runtime executes goals through investigate → plan → execute → verify.'));
  assert.deepEqual(view, { type: 'text', text: 'The FRIDAY runtime executes goals through investigate → plan → execute → verify.' });
});

test('an absent or empty result is an explicit empty view', () => {
  assert.deepEqual(classifyResult(undefined), { type: 'empty' });
  assert.deepEqual(classifyResult({ kind: 'empty', text: '' }), { type: 'empty' });
});

test('an error result carries its message', () => {
  const view = classifyResult({ kind: 'error', text: 'read_dir failed', success: false });
  assert.deepEqual(view, { type: 'error', text: 'read_dir failed' });
});

test('filesystem listing (JSON string inside a text result) becomes a file list', () => {
  const view = classifyResult(filesystemListing);
  assert.equal(view.type, 'fileList');
  if (view.type !== 'fileList') return;
  assert.equal(view.path, '.');
  assert.equal(view.entries.length, 5);
  assert.deepEqual(view.entries[0], { name: '.anchored-summary.md', is_dir: false });
});

test('isFileListing rejects non-listing shapes', () => {
  assert.equal(isFileListing(null), false);
  assert.equal(isFileListing([{ name: 'a' }]), false);
  assert.equal(isFileListing({ entries: 'nope' }), false);
  assert.equal(isFileListing({ entries: [{ path: 'no-name' }] }), false);
  assert.equal(isFileListing({ entries: [] }), true);
});

test('file list lines mark folders vs files', () => {
  const view = classifyResult(filesystemListing);
  assert.equal(view.type, 'fileList');
  if (view.type !== 'fileList') return;
  const lines = fileListLines(view.entries);
  assert.equal(lines.length, 5);
  assert.equal(lines[2].icon, 'folder');
  assert.equal(lines[2].name, '.git');
  assert.equal(lines[0].icon, 'file');
  assert.equal(isDirEntry({ name: 'a', is_dir: true }), true);
  assert.equal(isDirEntry({ name: 'b', isDir: true }), true);
  assert.equal(isDirEntry({ name: 'c', is_dir: false }), false);
});

test('entry count label is pluralised', () => {
  assert.equal(entryCountLabel(1), '1 entry');
  assert.equal(entryCountLabel(10), '10 entries');
});

test('an array result renders readable lines', () => {
  const view = classifyResult(textResult('["Cargo.toml","frontend","services"]'));
  assert.equal(view.type, 'array');
  if (view.type !== 'array') return;
  assert.deepEqual(view.items, ['Cargo.toml', 'frontend', 'services']);
  assert.deepEqual(arrayLines(view.items), ['Cargo.toml', 'frontend', 'services']);
});

test('array of objects collapses to scalar rows', () => {
  const lines = arrayLines([{ name: 'Cargo.toml', is_dir: false }, { name: 'services', is_dir: true }]);
  assert.deepEqual(lines, ['name: Cargo.toml · is_dir: false', 'name: services · is_dir: true']);
});

test('an unknown structured object becomes a human-readable view', () => {
  const view = classifyResult(
    textResult(JSON.stringify({ provider: 'openrouter', model: 'nemotron', latency_ms: 812 })),
  );
  assert.equal(view.type, 'structured');
  if (view.type !== 'structured') return;
  assert.deepEqual(structureRows(view.data), [
    { key: 'provider', value: 'openrouter' },
    { key: 'model', value: 'nemotron' },
    { key: 'latency_ms', value: '812' },
  ]);
});

test('nested structured values are summarised, not dumped', () => {
  const rows = structureRows({ name: 'report', meta: { a: 1 }, tags: ['x', 'y'], empty: [] });
  assert.deepEqual(rows, [
    { key: 'name', value: 'report' },
    { key: 'meta', value: '1 field' },
    { key: 'tags', value: 'x, y' },
    { key: 'empty', value: '[]' },
  ]);
});

test('a structured kind with an explicit payload prefers it over the text', () => {
  const view = classifyResult({
    kind: 'structured',
    text: 'ignored text',
    structured: { entries: [{ name: 'a.txt', is_dir: false }] },
    success: true,
  });
  assert.equal(view.type, 'fileList');
});

test('a structured kind with a scalar payload falls back to raw', () => {
  const view = classifyResult({ kind: 'structured', text: '42', structured: 42, success: true });
  assert.deepEqual(view, { type: 'raw', data: 42 });
});

test('plain numeric text is text, not a parsed number blob', () => {
  const view = classifyResult(textResult('83810205'));
  assert.deepEqual(view, { type: 'text', text: '83810205' });
});

test('tryParseJson tolerates non-JSON and empty input', () => {
  assert.equal(tryParseJson('not json'), null);
  assert.equal(tryParseJson('  '), null);
  assert.equal(tryParseJson(''), null);
  assert.deepEqual(tryParseJson('{"a":1}'), { a: 1 });
});
