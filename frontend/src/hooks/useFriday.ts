import { createContext, useContext } from 'react';
import { FridayClient } from '../protocol/client';

export const FridayContext = createContext<FridayClient | null>(null);

export function useFriday(): FridayClient {
  const ctx = useContext(FridayContext);
  if (!ctx) throw new Error('useFriday must be used within FridayProvider');
  return ctx;
}
