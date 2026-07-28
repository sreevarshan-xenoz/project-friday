import { Card, SectionLabel } from '../components';
import { useEvents } from '../hooks/useProtocol';
import './MemoryScreen.css';

export function MemoryScreen() {
  const events = useEvents(100);

  // Group events by day
  const grouped = groupByDay(events);

  return (
    <div className="memory">
      <h2 className="memory-title">Memory</h2>
      <p className="memory-subtitle">
        FRIDAY's experience timeline — what it remembers across sessions.
      </p>

      {grouped.length === 0 ? (
        <Card padding="lg" className="memory-empty">
          <div className="memory-empty-icon">◆</div>
          <div>No memories yet.</div>
          <div className="memory-empty-hint">
            As FRIDAY executes tasks, it builds procedural memory of what worked,
            what failed, and what to try next.
          </div>
        </Card>
      ) : (
        <div className="memory-timeline">
          {grouped.map((group) => (
            <div key={group.date} className="memory-day">
              <div className="memory-day-label">{group.date}</div>
              <div className="memory-day-line" />
              <div className="memory-day-events">
                {group.events.map((e) => (
                  <Card key={e.id} className="memory-entry" padding="sm">
                    <div className="memory-entry-header">
                      <span className="memory-entry-topic">{e.topic}</span>
                      <span className="memory-entry-time">
                        {new Date(e.timestamp).toLocaleTimeString([], {
                          hour: '2-digit',
                          minute: '2-digit',
                        })}
                      </span>
                    </div>
                    <div className="memory-entry-summary">{e.parsed.summary}</div>
                    {e.parsed.source && (
                      <div className="memory-entry-source">
                        source: {e.parsed.source}
                      </div>
                    )}
                  </Card>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

interface DayGroup {
  date: string;
  events: Array<{ id: string; topic: string; parsed: { summary: string; source: string }; timestamp: number }>;
}

function groupByDay(events: Array<{ id: string; topic: string; parsed: { summary: string; source: string }; timestamp: number }>): DayGroup[] {
  const map = new Map<string, DayGroup>();

  for (const evt of events) {
    const d = new Date(evt.timestamp);
    const key = d.toLocaleDateString(undefined, { weekday: 'long', month: 'short', day: 'numeric' });
    if (!map.has(key)) map.set(key, { date: key, events: [] });
    map.get(key)!.events.push(evt);
  }

  return Array.from(map.values());
}
