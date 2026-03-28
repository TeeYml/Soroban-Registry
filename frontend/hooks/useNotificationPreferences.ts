'use client';

import { useState, useEffect, useCallback } from 'react';
import { NotificationPreferences } from '@/types/realtime';

const DEFAULT_PREFERENCES: NotificationPreferences = {
  enableDeploymentNotifications: true,
  enableUpdateNotifications: true,
  enableDesktopNotifications: true,
  enableSoundNotification: false,
  updateTypes: ['verification_status', 'security_audit'],
};

const STORAGE_KEY = 'notification-preferences';

export function useNotificationPreferences() {
  const [preferences, setPreferences] = useState<NotificationPreferences>(DEFAULT_PREFERENCES);
  const [isLoaded, setIsLoaded] = useState(false);

  useEffect(() => {
    // Load from localStorage on mount
    if (typeof window !== 'undefined') {
      const stored = localStorage.getItem(STORAGE_KEY);
      if (stored) {
        try {
          setPreferences(JSON.parse(stored));
        } catch (error) {
          console.error('Failed to parse notification preferences:', error);
        }
      }
      setIsLoaded(true);
    }
  }, []);

  const updatePreferences = useCallback((updates: Partial<NotificationPreferences>) => {
    setPreferences(prev => {
      const updated = { ...prev, ...updates };
      localStorage.setItem(STORAGE_KEY, JSON.stringify(updated));
      return updated;
    });
  }, []);

  const toggleDeploymentNotifications = useCallback(() => {
    updatePreferences({
      enableDeploymentNotifications: !preferences.enableDeploymentNotifications,
    });
  }, [preferences.enableDeploymentNotifications, updatePreferences]);

  const toggleUpdateNotifications = useCallback(() => {
    updatePreferences({
      enableUpdateNotifications: !preferences.enableUpdateNotifications,
    });
  }, [preferences.enableUpdateNotifications, updatePreferences]);

  const toggleDesktopNotifications = useCallback(() => {
    updatePreferences({
      enableDesktopNotifications: !preferences.enableDesktopNotifications,
    });
  }, [preferences.enableDesktopNotifications, updatePreferences]);

  const toggleSoundNotification = useCallback(() => {
    updatePreferences({
      enableSoundNotification: !preferences.enableSoundNotification,
    });
  }, [preferences.enableSoundNotification, updatePreferences]);

  const updateUpdateTypes = useCallback((types: string[]) => {
    updatePreferences({ updateTypes: types });
  }, [updatePreferences]);

  return {
    preferences,
    isLoaded,
    updatePreferences,
    toggleDeploymentNotifications,
    toggleUpdateNotifications,
    toggleDesktopNotifications,
    toggleSoundNotification,
    updateUpdateTypes,
  };
}
