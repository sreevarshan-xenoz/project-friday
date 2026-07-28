import { FridayClient } from './protocol/client';
import { FridayContext } from './hooks/useFriday';
import { NavShell } from './NavShell';

// Default to localhost daemon; in production Tauri can inject the URL
const WS_URL = import.meta.env.VITE_FRIDAY_WS ?? 'ws://localhost:18432';
const TOKEN = localStorage.getItem('friday-token');

const client = new FridayClient(WS_URL, TOKEN ?? undefined);

// Auto-connect on load
client.connect();

// Persist token on auth
const origOnState = client.onState.bind(client);
let currentToken = TOKEN;
client.onState((state) => {
  origOnState(state);
});

export default function App() {
  return (
    <FridayContext.Provider value={client}>
      <NavShell />
    </FridayContext.Provider>
  );
}
