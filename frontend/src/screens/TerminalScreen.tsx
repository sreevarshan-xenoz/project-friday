import { useState } from 'react';
import { Terminal as TerminalComponent, Input } from '../components';
import { useFriday } from '../hooks/useFriday';
import { useEvents } from '../hooks/useProtocol';
import './TerminalScreen.css';

interface TermLine {
  prefix?: string;
  text: string;
  color?: string;
}

export function TerminalScreen() {
  const client = useFriday();
  const events = useEvents(50);
  const [input, setInput] = useState('');
  const [history, setHistory] = useState<TermLine[]>([
    { text: 'FRIDAY Terminal — connected to PC daemon', color: 'var(--fr-text-muted)' },
    { text: 'Type a command or goal. Press Enter to send.', color: 'var(--fr-text-muted)' },
    { text: '', color: 'var(--fr-text-muted)' },
  ]);

  // Append new events to terminal
  const eventLines: TermLine[] = events.slice(0, 5).map((e) => ({
    prefix: '←',
    text: `${e.topic}: ${e.parsed.summary}`,
    color: 'var(--fr-text-dim)',
  }));

  const allLines = [...history, ...eventLines];

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!input.trim()) return;

    const cmd = input.trim();
    setHistory((prev) => [
      ...prev,
      { prefix: '→', text: cmd, color: 'var(--fr-text)' },
    ]);

    client.sendGoal(cmd);
    setInput('');
  };

  return (
    <div className="terminal-screen">
      <div className="terminal-screen-header">
        <h2 className="terminal-screen-title">Terminal</h2>
      </div>
      <div className="terminal-screen-body">
        <TerminalComponent lines={allLines} />
      </div>
      <form className="terminal-screen-input" onSubmit={handleSubmit}>
        <Input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder="> Run cargo test"
          icon="$"
        />
      </form>
    </div>
  );
}
