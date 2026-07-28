import { useState, useRef, useEffect } from 'react';
import { AnimatePresence } from 'framer-motion';
import { ChatBubble, ResponseCard, ResponseLine, Input } from '../components';
import { useFriday } from '../hooks/useFriday';
import { useEvents, useCommandResults } from '../hooks/useProtocol';
import type { FridayEvent } from '../protocol';
import './ChatScreen.css';

interface ChatMessage {
  id: string;
  role: 'user' | 'friday';
  text?: string;
  event?: FridayEvent;
  commandResult?: { id: string; ok: boolean; result: unknown };
  timestamp: number;
}

export function ChatScreen() {
  const client = useFriday();
  const events = useEvents(100);
  const results = useCommandResults();
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState('');
  const bottomRef = useRef<HTMLDivElement>(null);

  // Sync events into chat messages
  useEffect(() => {
    setMessages((prev) => {
      const existing = new Set(prev.map((m) => m.id));
      const newMsgs: ChatMessage[] = [];

      for (const evt of events) {
        if (!existing.has(evt.id)) {
          newMsgs.push({
            id: evt.id,
            role: 'friday',
            event: evt,
            timestamp: evt.timestamp,
          });
        }
      }

      return newMsgs.length > 0
        ? [...newMsgs, ...prev].sort((a, b) => b.timestamp - a.timestamp)
        : prev;
    });
  }, [events]);

  // Sync command results into chat
  useEffect(() => {
    setMessages((prev) => {
      const existing = new Set(prev.map((m) => m.id));
      const newMsgs: ChatMessage[] = [];

      for (const r of results) {
        const id = `result-${r.id}`;
        if (!existing.has(id)) {
          newMsgs.push({
            id,
            role: 'friday',
            commandResult: r,
            timestamp: Date.now(),
          });
        }
      }

      return newMsgs.length > 0
        ? [...newMsgs, ...prev].sort((a, b) => b.timestamp - a.timestamp)
        : prev;
    });
  }, [results]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages.length]);

  const handleSend = (e: React.FormEvent) => {
    e.preventDefault();
    if (!input.trim()) return;

    const userMsg: ChatMessage = {
      id: `user-${Date.now()}`,
      role: 'user',
      text: input.trim(),
      timestamp: Date.now(),
    };

    setMessages((prev) => [userMsg, ...prev]);
    client.sendGoal(input.trim());
    setInput('');
  };

  return (
    <div className="chat">
      <div className="chat-header">
        <h2 className="chat-title">Assistant</h2>
      </div>

      <div className="chat-messages">
        {messages.length === 0 && (
          <div className="chat-empty">
            <div className="chat-empty-icon">›</div>
            <div>Send a command to get started.</div>
          </div>
        )}

        <AnimatePresence mode="popLayout">
          {messages.map((msg) => (
            <div key={msg.id} className={`chat-row chat-row--${msg.role}`}>
              {msg.role === 'user' ? (
                <ChatBubble role="user">{msg.text}</ChatBubble>
              ) : msg.event ? (
                <EventCard event={msg.event} />
              ) : msg.commandResult ? (
                <ResultCard result={msg.commandResult} />
              ) : null}
            </div>
          ))}
        </AnimatePresence>

        <div ref={bottomRef} />
      </div>

      <form className="chat-input" onSubmit={handleSend}>
        <Input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder="Send a command..."
          icon="›"
        />
      </form>
    </div>
  );
}

function EventCard({ event }: { event: FridayEvent }) {
  const { parsed } = event;

  if (parsed.task) {
    const task = parsed.task;
    const statusIcon = task.status === 'done' ? '✓' : task.status === 'running' ? '●' : task.status === 'failed' ? '✕' : '○';
    const statusColor =
      task.status === 'done' ? 'var(--fr-green)' :
      task.status === 'running' ? 'var(--fr-primary)' :
      task.status === 'failed' ? 'var(--fr-red)' :
      'var(--fr-text-muted)';

    return (
      <ResponseCard icon={statusIcon} title={event.topic}>
        <ResponseLine icon="" text={task.title} color={statusColor} />
        {task.source && (
          <ResponseLine icon="›" text={`source: ${task.source}`} color="var(--fr-text-dim)" />
        )}
      </ResponseCard>
    );
  }

  return (
    <ResponseCard title={event.topic}>
      <ResponseLine icon="›" text={parsed.summary} color="var(--fr-text-dim)" />
    </ResponseCard>
  );
}

function ResultCard({ result }: { result: { id: string; ok: boolean; result: unknown } }) {
  const icon = result.ok ? '✓' : '✕';
  const color = result.ok ? 'var(--fr-green)' : 'var(--fr-red)';
  const text = typeof result.result === 'string'
    ? result.result
    : JSON.stringify(result.result, null, 2);

  return (
    <ResponseCard icon={icon} title={`Result ${result.id}`}>
      <ResponseLine icon="" text={text} color={color} />
    </ResponseCard>
  );
}
