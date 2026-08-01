/**
 * FRIDAY Engineering Console — reactive store.
 *
 * Module-level store exposing the console read models. Currently seeded from
 * the deterministic mock generator; a real bridge (WebSocket/Tauri) replaces
 * the seed function later without touching the screens, which only subscribe
 * via `useConsole`.
 */

import { useSyncExternalStore } from 'react';
import { generateConsoleState, deriveOverview, type ConsoleState, type Overview } from './mock';

export interface ConsoleSnapshot {
  state: ConsoleState;
  overview: Overview;
}

let snapshot: ConsoleSnapshot;
const listeners = new Set<() => void>();

export function seedConsole(): ConsoleSnapshot {
  const state = generateConsoleState(140, 42);
  snapshot = { state, overview: deriveOverview(state) };
  return snapshot;
}

seedConsole();

export function getConsoleSnapshot(): ConsoleSnapshot {
  return snapshot;
}

export function subscribeConsole(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function refreshConsole(): void {
  const state = generateConsoleState(140, 42);
  snapshot = { state, overview: deriveOverview(state) };
  listeners.forEach((l) => l());
}

export function useConsole(): ConsoleSnapshot {
  return useSyncExternalStore(subscribeConsole, getConsoleSnapshot);
}
