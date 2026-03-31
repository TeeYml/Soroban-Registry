'use client';

import { useState, useEffect, useCallback } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { api, AnalyticsEvent, AnalyticsEventType, ActivityFeedResponse } from '@/lib/api';
import { useRealtime } from '@/hooks/useRealtime';
import { 
  Activity, 
  Upload, 
  CheckCircle2, 
  RefreshCcw, 
  UserPlus, 
  ChevronDown, 
  ExternalLink,
  Filter,
  Clock,
  Zap,
  Tag
} from 'lucide-react';
import Link from 'next/link';

const EVENT_CONFIG: Record<string, { icon: any, label: string, color: string }> = {
  contract_published: { icon: Upload, label: 'Published', color: 'text-blue-500 bg-blue-500/10' },
  contract_verified: { icon: CheckCircle2, label: 'Verified', color: 'text-emerald-500 bg-emerald-500/10' },
  contract_deployed: { icon: Zap, label: 'Deployed', color: 'text-amber-500 bg-amber-500/10' },
  version_created: { icon: RefreshCcw, label: 'New Version', color: 'text-purple-500 bg-purple-500/10' },
  contract_updated: { icon: Tag, label: 'Updated', color: 'text-indigo-500 bg-indigo-500/10' },
  publisher_created: { icon: UserPlus, label: 'New Publisher', color: 'text-pink-500 bg-pink-500/10' },
};

export default function ActivityFeed() {
  const queryClient = useQueryClient();
  const { subscribe, isConnected } = useRealtime();

  const [eventType, setEventType] = useState<AnalyticsEventType | 'all'>('all');
  const [items, setItems] = useState<AnalyticsEvent[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [isFetchingMore, setIsFetchingMore] = useState(false);

  // Initial fetch
  const { isLoading, error, data } = useQuery<ActivityFeedResponse>({
    queryKey: ['activity-feed', eventType],
    queryFn: () => api.getActivityFeed({ 
      event_type: eventType === 'all' ? undefined : eventType,
      limit: 20
    }),
  });

  // Update items when data changes
  useEffect(() => {
    if (data) {
      setItems(data.items);
      setNextCursor(data.next_cursor);
    }
  }, [data]);

  // Handle real-time events
  useEffect(() => {
    const handleDeployment = (event: any) => {
      // Convert RealtimeEvent to AnalyticsEvent
      const newEvent: AnalyticsEvent = {
        id: Math.random().toString(36).substring(7),
        event_type: 'contract_deployed',
        contract_id: event.contract_id,
        user_address: event.publisher,
        network: null, // We don't have it in the realtime event directly but could infer or leave null
        metadata: { name: event.contract_name, version: event.version },
        created_at: event.timestamp || new Date().toISOString(),
      };

      if (eventType === 'all' || eventType === 'contract_deployed') {
        setItems(prev => [newEvent, ...prev].slice(0, 100)); // Limit local cache
      }
    };

    const handleUpdate = (event: any) => {
      const newEvent: AnalyticsEvent = {
        id: Math.random().toString(36).substring(7),
        event_type: 'contract_updated',
        contract_id: event.contract_id,
        user_address: null,
        network: null,
        metadata: { update_type: event.update_type, ...event.details },
        created_at: event.timestamp || new Date().toISOString(),
      };

      if (eventType === 'all' || eventType === 'contract_updated') {
        setItems(prev => [newEvent, ...prev].slice(0, 100));
      }
    };

    const unsubDeploy = subscribe('contract_deployed', handleDeployment);
    const unsubUpdate = subscribe('contract_updated', handleUpdate);

    return () => {
      unsubDeploy();
      unsubUpdate();
    };
  }, [subscribe, eventType]);

  const fetchMore = async () => {
    if (!nextCursor || isFetchingMore) return;
    setIsFetchingMore(true);
    try {
      const res = await api.getActivityFeed({
        cursor: nextCursor,
        event_type: eventType === 'all' ? undefined : eventType,
        limit: 20
      });
      setItems(prev => [...prev, ...res.items]);
      setNextCursor(res.next_cursor);
    } catch (err) {
      console.error('Failed to fetch more activity:', err);
    } finally {
      setIsFetchingMore(false);
    }
  };

  const formatAddress = (addr: string | null) => {
    if (!addr) return 'Unknown';
    if (addr.length < 12) return addr;
    return `${addr.slice(0, 6)}...${addr.slice(-4)}`;
  };

  const formatTime = (dateStr: string) => {
    const date = new Date(dateStr);
    const now = new Date();
    const diff = now.getTime() - date.getTime();
    
    if (diff < 60000) return 'just now';
    if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`;
    if (diff < 86400000) return `${Math.floor(diff / 3600000)}h ago`;
    return date.toLocaleDateString();
  };

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Activity className="w-6 h-6 text-primary" />
          <h2 className="text-xl font-bold text-foreground">Registry Activity</h2>
          {isConnected && (
            <span className="flex h-2 w-2 rounded-full bg-emerald-500 animate-pulse ml-1" title="Live updates active" />
          )}
        </div>

        <div className="flex items-center gap-2 bg-card border border-border rounded-lg px-3 py-1.5 shadow-sm">
          <Filter className="w-4 h-4 text-muted-foreground" />
          <select 
            value={eventType}
            onChange={(e) => setEventType(e.target.value as any)}
            className="bg-transparent text-sm font-medium text-foreground focus:outline-none cursor-pointer"
          >
            <option value="all">All Events</option>
            <option value="contract_published">Published</option>
            <option value="contract_verified">Verified</option>
            <option value="contract_deployed">Deployed</option>
            <option value="version_created">New Version</option>
            <option value="contract_updated">Updated</option>
          </select>
        </div>
      </div>

      <div className="bg-card border border-border rounded-xl overflow-hidden shadow-sm">
        {isLoading && items.length === 0 ? (
          <div className="p-12 flex flex-col items-center justify-center text-muted-foreground gap-3">
            <RefreshCcw className="w-8 h-8 animate-spin text-primary/40" />
            <p className="text-sm">Loading activity feed...</p>
          </div>
        ) : error ? (
          <div className="p-12 text-center text-red-500">
            <p>Failed to load activity feed.</p>
          </div>
        ) : items.length === 0 ? (
          <div className="p-12 text-center text-muted-foreground">
            <p>No recent activity found.</p>
          </div>
        ) : (
          <div className="divide-y divide-border">
            {items.map((item) => {
              const config = EVENT_CONFIG[item.event_type] || { icon: Activity, label: item.event_type, color: 'text-gray-500 bg-gray-500/10' };
              const Icon = config.icon;
              
              return (
                <div key={item.id} className="p-4 hover:bg-muted/30 transition-colors group">
                  <div className="flex gap-4">
                    <div className={`mt-1 p-2 rounded-full h-fit ${config.color}`}>
                      <Icon className="w-4 h-4" />
                    </div>
                    
                    <div className="flex-1 flex flex-col gap-1">
                      <div className="flex items-center justify-between gap-2">
                        <div className="flex flex-wrap items-center gap-x-2 text-sm">
                          <span className="font-semibold text-foreground uppercase text-[10px] tracking-wider px-1.5 py-0.5 rounded border border-border bg-muted/50">
                            {config.label}
                          </span>
                          <Link 
                            href={`/contracts/${item.contract_id}`}
                            className="font-medium text-primary hover:underline flex items-center gap-1"
                          >
                            {item.metadata?.name || item.contract_id.slice(0, 10) + '...'}
                            <ExternalLink className="w-3 h-3 opacity-0 group-hover:opacity-100 transition-opacity" />
                          </Link>
                          {item.metadata?.version && (
                            <span className="text-muted-foreground">v{item.metadata.version}</span>
                          )}
                        </div>
                        <div className="flex items-center gap-1 text-xs text-muted-foreground whitespace-nowrap">
                          <Clock className="w-3 h-3" />
                          {formatTime(item.created_at)}
                        </div>
                      </div>

                      <div className="text-sm text-muted-foreground flex flex-wrap items-center gap-x-2">
                        {item.user_address && (
                          <>
                            <span>by</span>
                            <span className="font-mono text-foreground bg-muted px-1 rounded text-[11px]">
                              {formatAddress(item.user_address)}
                            </span>
                          </>
                        )}
                        {item.network && (
                          <span className="capitalize px-1.5 py-0.5 rounded-full bg-primary/5 text-primary text-[10px] font-bold">
                            {item.network}
                          </span>
                        )}
                      </div>

                      {item.event_type === 'contract_updated' && item.metadata?.update_type && (
                        <div className="mt-1 text-xs px-2 py-1 rounded bg-muted/50 border border-border inline-block w-fit">
                          <span className="font-medium">Type:</span> {item.metadata.update_type}
                        </div>
                      )}
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {nextCursor && (
        <button
          onClick={fetchMore}
          disabled={isFetchingMore}
          className="flex items-center justify-center gap-2 py-2 px-4 rounded-lg border border-border hover:bg-muted font-medium text-sm transition-colors text-foreground disabled:opacity-50"
        >
          {isFetchingMore ? (
            <RefreshCcw className="w-4 h-4 animate-spin" />
          ) : (
            <ChevronDown className="w-4 h-4" />
          )}
          Load More Activity
        </button>
      )}
    </div>
  );
}
