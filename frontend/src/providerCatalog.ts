/**
 * Provider catalog — routing *intent* options for the composer and the
 * Providers screen. A read-only mirror of the Rust `ProviderId` registry
 * (`services/action-runtime/src/adapters/provider.rs`); the runtime owns the
 * actual expert selection.
 */

export interface ProviderOption {
  id: string;
  label: string;
  transport: 'Local' | 'Browser' | 'API';
  detail: string;
}

export const PROVIDER_OPTIONS: ProviderOption[] = [
  { id: 'auto', label: 'Auto', transport: 'API', detail: 'Runtime-routed — FRIDAY picks the best expert' },
  { id: 'local', label: 'Local · Gemma', transport: 'Local', detail: 'llama.cpp / Ollama — on-machine, private' },
  { id: 'chatgpt', label: 'ChatGPT', transport: 'Browser', detail: 'Deep reasoning' },
  { id: 'grok', label: 'Grok', transport: 'Browser', detail: 'Realtime' },
  { id: 'gemini', label: 'Gemini', transport: 'Browser', detail: 'Vision' },
  { id: 'claude', label: 'Claude', transport: 'Browser', detail: 'Coding + citations' },
  { id: 'perplexity', label: 'Perplexity', transport: 'Browser', detail: 'Realtime + citations' },
];

export function providerById(id: string): ProviderOption {
  return PROVIDER_OPTIONS.find((p) => p.id === id) ?? PROVIDER_OPTIONS[0];
}
