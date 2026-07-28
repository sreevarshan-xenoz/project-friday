import { useRef, useEffect } from 'react';
import './Terminal.css';

interface TerminalLine {
  prefix?: string;
  text: string;
  color?: string;
}

interface TerminalProps {
  lines: TerminalLine[];
  className?: string;
}

export function Terminal({ lines, className = '' }: TerminalProps) {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [lines.length]);

  return (
    <div className={`fr-terminal ${className}`}>
      <div className="fr-terminal-header">
        <span className="fr-terminal-dot" style={{ background: '#FF5F57' }} />
        <span className="fr-terminal-dot" style={{ background: '#FFBD2E' }} />
        <span className="fr-terminal-dot" style={{ background: '#28CA41' }} />
        <span className="fr-terminal-title">terminal</span>
      </div>
      <div className="fr-terminal-body">
        {lines.map((line, i) => (
          <div key={i} className="fr-terminal-line">
            {line.prefix && (
              <span className="fr-terminal-prefix">{line.prefix}</span>
            )}
            <span style={{ color: line.color ?? 'var(--fr-text-dim)' }}>{line.text}</span>
          </div>
        ))}
        <div ref={bottomRef} />
      </div>
    </div>
  );
}
