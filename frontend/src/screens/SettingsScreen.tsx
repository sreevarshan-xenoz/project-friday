import { useState } from 'react';
import { Card, SectionLabel, Button, Input, StatusPill } from '../components';
import { useFriday } from '../hooks/useFriday';
import { useConnection } from '../hooks/useProtocol';
import './SettingsScreen.css';

export function SettingsScreen() {
  const client = useFriday();
  const conn = useConnection();
  const [url, setUrl] = useState('ws://localhost:18432');

  const handleConnect = () => {
    client.disconnect();
    // In a real app, we'd recreate the client with the new URL
  };

  return (
    <div className="settings">
      <h2 className="settings-title">Settings</h2>

      <div className="settings-sections">
        <Card padding="lg">
          <SectionLabel>Connection</SectionLabel>
          <div className="settings-field">
            <label className="settings-label">PC WebSocket URL</label>
            <Input
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="ws://localhost:18432"
            />
          </div>
          <div className="settings-actions">
            <Button onClick={handleConnect}>Connect</Button>
            <Button variant="danger" onClick={() => client.disconnect()}>
              Disconnect
            </Button>
          </div>
        </Card>

        <Card padding="lg">
          <SectionLabel>Status</SectionLabel>
          <div className="settings-status-grid">
            <div className="settings-status-row">
              <span className="settings-status-label">Connection</span>
              <StatusPill
                level={conn.status === 'connected' ? 'green' : 'red'}
                text={conn.status}
              />
            </div>
            <div className="settings-status-row">
              <span className="settings-status-label">Session</span>
              <span className="settings-status-value mono">
                {conn.sessionId?.slice(0, 8) ?? '—'}
              </span>
            </div>
            <div className="settings-status-row">
              <span className="settings-status-label">Protocol</span>
              <span className="settings-status-value mono">{conn.protocol}</span>
            </div>
            <div className="settings-status-row">
              <span className="settings-status-label">Last error</span>
              <span className="settings-status-value" style={{ color: conn.lastError ? 'var(--fr-red)' : undefined }}>
                {conn.lastError ?? '—'}
              </span>
            </div>
          </div>
        </Card>

        <Card padding="lg">
          <SectionLabel>About</SectionLabel>
          <div className="settings-about">
            <div className="settings-about-row">
              <span>FRIDAY Desktop</span>
              <span className="settings-status-value">v0.1.0</span>
            </div>
            <div className="settings-about-row">
              <span>Build</span>
              <span className="settings-status-value mono">alpha</span>
            </div>
          </div>
        </Card>
      </div>
    </div>
  );
}
