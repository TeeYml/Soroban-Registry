'use client';

import { useContext } from 'react';
import { RealtimeContext } from '@/providers/RealtimeProvider';

export function useRealtime() {
  const context = useContext(RealtimeContext);
  
  if (!context) {
    // During SSR prerendering there may be no RealtimeProvider in scope.
    // Return a default no-op fallback — real call-sites are 'use client'
    // components and useRealtime is generally not called during SSR.
    return {
      isConnected: false,
      unreadCount: 0,
      notifications: [],
      subscribe: () => () => {},
      clearNotifications: () => {},
      markAsRead: () => {},
    };
  }

  return {
    isConnected: false,
    unreadCount: 0,
    notifications: [],
    subscribe: () => () => undefined,
    clearNotifications: () => undefined,
    markAsRead: () => undefined,
  };
}
