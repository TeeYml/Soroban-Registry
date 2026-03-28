'use client';

import { createContext, useCallback, useEffect, useState, ReactNode } from 'react';
import { wsService } from '@/services/websocket.service';
import { ContractDeploymentEvent, RealtimeContextType } from '@/types/realtime';
import { requestDesktopNotification, sendDesktopNotification } from '@/utils/notifications';

export const RealtimeContext = createContext<RealtimeContextType | undefined>(undefined);

interface RealtimeProviderProps {
  children: ReactNode;
}

export default function RealtimeProvider({ children }: RealtimeProviderProps) {
  const [isConnected, setIsConnected] = useState(false);
  const [unreadCount, setUnreadCount] = useState(0);
  const [notifications, setNotifications] = useState<ContractDeploymentEvent[]>([]);

  useEffect(() => {
    // Request desktop notification permission on mount
    requestDesktopNotification();

    // Connect WebSocket
    wsService.connect().catch(console.error);

    // Handle connection
    const unsubscribeOpen = wsService.onOpen(() => {
      setIsConnected(true);
    });

    // Handle deployment events
    const unsubscribeDeploy = wsService.on('contract_deployed', (data: ContractDeploymentEvent) => {
      setNotifications(prev => [data, ...prev].slice(0, 50)); // Keep last 50
      setUnreadCount(prev => prev + 1);

      // Show desktop notification if enabled
      const preferences = localStorage.getItem('notification-preferences');
      if (preferences) {
        const prefs = JSON.parse(preferences);
        if (prefs.enableDesktopNotifications && prefs.enableDeploymentNotifications) {
          sendDesktopNotification({
            title: 'New Contract Deployed',
            options: {
              body: `${data.contractName} v${data.version} by ${data.publisher}`,
              tag: `contract-${data.contractId}`,
              requireInteraction: false,
            },
          });
        }
      }
    });

    // Handle update events
    const unsubscribeUpdate = wsService.on('contract_updated', (data: any) => {
      const preferences = localStorage.getItem('notification-preferences');
      if (preferences) {
        const prefs = JSON.parse(preferences);
        if (prefs.enableDesktopNotifications && 
            prefs.enableUpdateNotifications &&
            prefs.updateTypes.includes(data.updateType)) {
          sendDesktopNotification({
            title: `Contract Update: ${data.updateType}`,
            options: {
              body: `Contract ${data.contractId} has been updated`,
              tag: `update-${data.contractId}`,
            },
          });
        }
      }
    });

    return () => {
      unsubscribeOpen();
      unsubscribeDeploy();
      unsubscribeUpdate();
      wsService.disconnect();
    };
  }, []);

  const subscribe = useCallback((type: string, handler: (data: any) => void) => {
    return wsService.on(type, handler);
  }, []);

  const clearNotifications = useCallback(() => {
    setNotifications([]);
    setUnreadCount(0);
  }, []);

  const markAsRead = useCallback((notificationId: string) => {
    if (unreadCount > 0) {
      setUnreadCount(prev => prev - 1);
    }
  }, [unreadCount]);

  const value: RealtimeContextType = {
    isConnected,
    unreadCount,
    notifications,
    subscribe,
    clearNotifications,
    markAsRead,
  };

  return (
    <RealtimeContext.Provider value={value}>
      {children}
    </RealtimeContext.Provider>
  );
}
