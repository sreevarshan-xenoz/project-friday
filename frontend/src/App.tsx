import { FridayClient } from './protocol/client';
import { FridayContext } from './hooks/useFriday';
import { NavShell } from './NavShell';

// Default to localhost daemon; in production Tauri can inject the URL
const WS_URL = import.meta.env.VITE_FRIDAY_WS ?? 'ws://localhost:18432';

const client = new FridayClient(WS_URL, localStorage.getItem('friday-token') ?? undefined);

// Auto-connect on load
client.connect();

export default function App() {
  return (
    <FridayContext.Provider value={client}>
      <NavShell />
    </FridayContext.Provider>
  );
}
