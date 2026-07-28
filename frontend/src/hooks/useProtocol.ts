import { useEffect, useState } from 'react';
import { useFriday } from './useFriday';
import type { ConnectionState, FridayEvent } from '../protocol';

/** Subscribe to the FridayClient's connection state. */
export function useConnection(): ConnectionState {
  const client = useFriday();
  const [state, setState] = useState<ConnectionState>(client.state);

  useEffect(() => {
    // Sync immediately in case state changed between render and effect
    setState(client.state);
    return client.onState(setState);
  }, [client]);

  return state;
}

/** Subscribe to incoming FRIDAY events. Returns events newest-first, capped. */
export function useEvents(maxCount = 50): FridayEvent[] {
  const client = useFriday();
  const [events, setEvents] = useState<FridayEvent[]>([]);

  useEffect(() => {
    return client.onEvents((evt) => {
      setEvents((prev) => [evt, ...prev].slice(0, maxCount));
    });
  }, [client, maxCount]);

  return events;
}

/** Subscribe to command results. */
export function useCommandResults() {
  const client = useFriday();
  const [results, setResults] = useState<Array<{ id: string; ok: boolean; result: unknown }>>([]);

  useEffect(() => {
    return client.onCommandResult((id, ok, result) => {
      setResults((prev) => [{ id, ok, result }, ...prev].slice(0, 30));
    });
  }, [client]);

  return results;
}
