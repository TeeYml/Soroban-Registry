'use client';

import { useState } from 'react';
import { useAnalytics } from '@/hooks/useAnalytics';

interface CopyOptions {
  successEventName?: string;
  failureEventName?: string;
  analyticsParams?: Record<string, unknown>;
}

export function useCopy() {
  const [copied, setCopied] = useState(false);
  const [isCopying, setIsCopying] = useState(false);
  const { logEvent } = useAnalytics();

  // Copies text to clipboard and tracks success/failure analytics in one place.
  const copy = async (text: string, options?: CopyOptions) => {
    if (!text) {
      return false;
    }

    setIsCopying(true);

    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);

      // Success event can be customized per usage (e.g., contract code copy).
      logEvent(options?.successEventName || 'code_copied', {
        ...options?.analyticsParams,
      });

      setTimeout(() => setCopied(false), 1800);
      return true;
    } catch {

      // Failure event keeps the same context payload for troubleshooting.
      logEvent(options?.failureEventName || 'code_copy_failed', {
        ...options?.analyticsParams,
      });

      return false;
    } finally {
      setIsCopying(false);
    }
  };

  return { copy, copied, isCopying };
}
