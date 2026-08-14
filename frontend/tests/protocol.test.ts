/**
 * FRIDAY Mobile Protocol — S3 human-control parse tests.
 *
 * Run: `npm test` (Node 24 native TS type-stripping + `node --test`; no
 * framework needed). Fixtures mirror the exact event payloads the daemon
 * publishes for cancel / pause / approve (plus the bus bridge `_kind`/
 * `_source` tags the portal relies on).
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';

import { parseEvent } from '../src/protocol/protocol.ts';

const bridged = (kind: string, extra: Record<string, unknown>) => ({
  _kind: kind,
  _source: 'planner',
  ...extra,
});

test('goal.cancelled parses as a cancelled task with the run id', () => {
  const ev = parseEvent(
    'goal.cancelled',
    bridged('goal.cancelled', {
      goal: 'Search the web for the latest Rust async docs',
      id: 'goal-abc123',
      session_id: 'sess-1',
      session_turn: 2,
      reason: 'cancelled_by_human',
      phases: 2,
    }),
  );
  assert.equal(ev.kind, 'goal.cancelled');
  assert.equal(ev.goalId, 'goal-abc123');
  assert.equal(ev.task?.status, 'cancelled');
  assert.equal(ev.task?.id, 'goal-abc123');
  assert.equal(ev.task?.title, 'Search the web for the latest Rust async docs');
  assert.equal(ev.phases, 2);
});

test('goal.paused parses as a paused task', () => {
  const ev = parseEvent(
    'goal.paused',
    bridged('goal.paused', {
      goal: 'Search the web for the latest Rust async docs',
      id: 'goal-def456',
      reason: 'paused_by_human',
      phases: 1,
    }),
  );
  assert.equal(ev.task?.status, 'paused');
  assert.equal(ev.goalId, 'goal-def456');
});

test('a held sensitive goal surfaces its pending approval id', () => {
  const ev = parseEvent(
    'goal.completed',
    bridged('goal.completed', {
      goal: 'Search the web for how to reset my password safely',
      id: 'goal-789',
      phases: 4,
      pending_approval: true,
      pending_approval_id: 'ep-held-42',
    }),
  );
  assert.equal(ev.task?.status, 'done');
  assert.equal(ev.pendingApproval, true);
  assert.equal(ev.pendingApprovalId, 'ep-held-42');
});

test('an approved goal carries the held episode id as task_id', () => {
  const ev = parseEvent(
    'goal.approved',
    bridged('goal.approved', { task_id: 'ep-held-42' }),
  );
  assert.equal(ev.kind, 'goal.approved');
  assert.equal(ev.taskId, 'ep-held-42');
  assert.equal(ev.task, null, 'goal.approved has no card status of its own');
});
