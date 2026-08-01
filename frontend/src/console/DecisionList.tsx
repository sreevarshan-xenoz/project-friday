import type { DecisionTimeline } from './types';
import { Chip } from './ui';
import { fmtTime } from './format';
import './DecisionList.css';

interface DecisionListProps {
  timelines: DecisionTimeline[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}

export function DecisionList({ timelines, selectedId, onSelect }: DecisionListProps) {
  return (
    <div className="con-dlist">
      {timelines.map((t) => (
        <button
          key={t.correlation_id}
          className={`con-dlist-item${t.correlation_id === selectedId ? ' con-dlist-item--active' : ''}`}
          onClick={() => onSelect(t.correlation_id)}
        >
          <div className="con-dlist-top">
            <span className="con-dlist-cap">{t.plan.capability}</span>
            <Chip tone={t.outcome.success ? 'green' : 'red'}>{t.outcome.success ? 'OK' : 'FAIL'}</Chip>
          </div>
          <div className="con-dlist-bottom">
            <span className="con-dlist-op">{t.plan.operator_id}</span>
            <span className="con-dlist-time">{fmtTime(t.record.timestamp_ms)}</span>
          </div>
        </button>
      ))}
    </div>
  );
}
