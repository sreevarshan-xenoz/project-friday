#!/usr/bin/env node
/**
 * Protocol Compatibility Suite — conformance checker.
 *
 * Validates the frozen golden fixtures in `protocol-tests/v1/` against the
 * wire contract in `services/bridge/PROTOCOL.md`. It is deliberately
 * framework-agnostic (plain Node, no dependencies) so ANY future bridge
 * implementation can vendor it as its parse gate: a new implementation must
 * parse every historical frame and produce these same shapes.
 *
 * Run:  node protocol-tests/check.mjs
 */

import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)));
const dir = join(root, 'v1');

const FRAME_KINDS = new Set(['hello', 'snapshot', 'delta', 'heartbeat', 'error', 'capabilities']);
const CAPABILITY_NAMES = new Set([
  'delta', 'heartbeat', 'resync', 'errors', 'metrics', 'replay', 'gzip', 'zstd', 'binary', 'auth',
]);
const ERROR_CODES = new Set([
  'unsupported_version', 'unsupported_capability', 'authentication_failed',
  'compression_negotiation_failed', 'invalid_frame', 'invalid_sequence', 'resync_required',
  'rate_limited', 'internal_bridge_error',
]);
const SEVERITIES = new Set(['info', 'warning', 'error', 'critical']);

let failures = 0;

function fail(name, detail) {
  failures += 1;
  console.error(`FAIL ${name} — ${detail}`);
}

function checkCommon(name, frame) {
  if (typeof frame.kind !== 'string') return fail(name, 'missing `kind`');
  if (!FRAME_KINDS.has(frame.kind)) return fail(name, `unknown kind ${frame.kind}`);
  if (!Number.isInteger(frame.sequence) || frame.sequence < 0) return fail(name, '`sequence` must be a non-negative integer');
  if (!Number.isInteger(frame.timestamp) || frame.timestamp <= 0) return fail(name, '`timestamp` must be a positive integer');
  if (typeof frame.source !== 'string') return fail(name, 'missing `source`');
  if (frame.protocol !== 1) return fail(name, `protocol must be 1, got ${frame.protocol}`);
  if (typeof frame.bridge !== 'string') return fail(name, 'missing `bridge`');
}

function checkCapabilitySet(name, path, set) {
  if (typeof set !== 'object' || set === null) return fail(name, `${path} must be an object`);
  if (set.protocol_version !== 1) return fail(name, `${path}.protocol_version must be 1`);
  for (const group of ['required', 'optional']) {
    const list = set[group];
    if (!Array.isArray(list)) return fail(name, `${path}.${group} must be an array`);
    for (const cap of list) {
      if (!CAPABILITY_NAMES.has(cap.name)) return fail(name, `${path}.${group} has unknown capability ${cap.name}`);
      if (!Number.isInteger(cap.version)) return fail(name, `${path}.${group}.${cap.name}.version must be an integer`);
      if (typeof cap.required !== 'boolean') return fail(name, `${path}.${group}.${cap.name}.required must be a boolean`);
    }
  }
}

function checkBridgeStats(name, stats) {
  if (typeof stats !== 'object' || stats === null) return fail(name, '`bridge_stats` must be an object');
  for (const key of ['connections', 'reconnects', 'resyncs', 'frames_sent', 'bytes_sent', 'snapshot_size_bytes', 'delta_size_bytes']) {
    if (!Number.isInteger(stats[key])) return fail(name, `bridge_stats.${key} must be an integer`);
  }
}

function validateFrame(name, frame) {
  checkCommon(name, frame);
  switch (frame.kind) {
    case 'hello':
      checkCapabilitySet(name, 'capabilities', frame.capabilities);
      if (frame.bridge_stats != null) checkBridgeStats(name, frame.bridge_stats);
      break;
    case 'capabilities':
      checkCapabilitySet(name, 'capabilities', frame.capabilities);
      break;
    case 'snapshot':
      if (typeof frame.dashboard !== 'object' || frame.dashboard === null) return fail(name, '`dashboard` must be an object');
      if (!Array.isArray(frame.timelines)) return fail(name, '`timelines` must be an array');
      if (typeof frame.metrics !== 'object' || frame.metrics === null) return fail(name, '`metrics` must be an object');
      if (!Array.isArray(frame.ledger)) return fail(name, '`ledger` must be an array');
      break;
    case 'delta':
      if (!['dashboard', 'timelines', 'metrics', 'ledger'].some((k) => frame[k] != null)) {
        return fail(name, '`delta` must carry at least one slice');
      }
      break;
    case 'heartbeat':
      if (!(frame.latency_ms === null || Number.isInteger(frame.latency_ms))) return fail(name, '`latency_ms` must be an integer or null');
      checkBridgeStats(name, frame.bridge_stats);
      break;
    case 'error':
      if (typeof frame.message !== 'string') return fail(name, '`message` must be a string');
      if (!ERROR_CODES.has(frame.code)) return fail(name, `unknown error code ${frame.code}`);
      if (!SEVERITIES.has(frame.severity)) return fail(name, `unknown severity ${frame.severity}`);
      if (typeof frame.recoverable !== 'boolean') return fail(name, '`recoverable` must be a boolean');
      if (typeof frame.recommended_action !== 'string') return fail(name, '`recommended_action` must be a string');
      break;
  }
}

function validateControl(name, doc) {
  if (typeof doc !== 'object' || doc === null) return fail(name, 'control message must be a JSON object');
  if (doc.type === 'hello_response') {
    if (typeof doc.capabilities !== 'object' || doc.capabilities === null) return fail(name, 'hello_response.capabilities must be an object');
    for (const [cap, enabled] of Object.entries(doc.capabilities)) {
      if (!CAPABILITY_NAMES.has(cap)) return fail(name, `hello_response selects unknown capability ${cap}`);
      if (typeof enabled !== 'boolean') return fail(name, `hello_response.capabilities.${cap} must be a boolean`);
    }
  } else if (doc.type === 'resync') {
    // no further fields
  } else {
    return fail(name, `unknown control type ${doc.type}`);
  }
}

const files = readdirSync(dir).filter((f) => f.endsWith('.json')).sort();
let checked = 0;
for (const file of files) {
  let doc;
  try {
    doc = JSON.parse(readFileSync(join(dir, file), 'utf8'));
  } catch (e) {
    fail(file, `not valid JSON: ${e.message}`);
    continue;
  }
  if (file.startsWith('control-')) {
    validateControl(file, doc);
  } else {
    validateFrame(file, doc);
  }
  checked += 1;
}

if (checked === 0) {
  console.error('FAIL — no fixtures found under protocol-tests/v1/');
  process.exit(1);
}

if (failures === 0) {
  console.log(`PASS — ${checked} fixtures conform to the frozen wire contract`);
  process.exit(0);
}
console.error(`FAIL — ${failures} conformance violation(s) across ${checked} fixtures`);
process.exit(1);
