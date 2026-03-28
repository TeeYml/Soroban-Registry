'use client';

import { useContext } from 'react';
import { RealtimeContext } from '@/providers/RealtimeProvider';

export function useRealtime() {
  const context = useContext(RealtimeContext);
  
  if (!context) {
    throw new Error('useRealtime must be used within a RealtimeProvider');
  }
  
  return context;
}
