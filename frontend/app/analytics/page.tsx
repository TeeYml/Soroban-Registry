'use client';

import React, { useState, useEffect, useCallback } from 'react';
import Navbar from '@/components/Navbar';
import TrendingContractsTable from '@/components/analytics/TrendingContractsTable';
import CategoryDistributionPie from '@/components/analytics/CategoryDistributionPie';
import DeploymentTrendGraph from '@/components/analytics/DeploymentTrendGraph';
import NetworkUsageBarChart from '@/components/analytics/NetworkUsageBarChart';
import RecentAdditionsTimeline from '@/components/analytics/RecentAdditionsTimeline';
import TopPublishersList from '@/components/analytics/TopPublishersList';
import { AlertCircle, RefreshCw, BarChart3, Users, Clock, Flame, PieChart, Activity } from 'lucide-react';

export default function AnalyticsDashboard() {
  const [data, setData] = useState<any>(null);
  const [trending, setTrending] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState(false);

  const fetchData = useCallback(async (isAutoRefresh = false) => {
    if (!isAutoRefresh) setLoading(true);
    else setRefreshing(true);
    
    setError(false);
    try {
      const [dashRes, trendRes] = await Promise.all([
        fetch(process.env.NEXT_PUBLIC_API_URL ? `${process.env.NEXT_PUBLIC_API_URL}/api/analytics/dashboard` : '/api/analytics/dashboard'),
        fetch(process.env.NEXT_PUBLIC_API_URL ? `${process.env.NEXT_PUBLIC_API_URL}/api/contracts/trending?limit=10` : '/api/contracts/trending?limit=10')
      ]);
      
      if (!dashRes.ok || !trendRes.ok) throw new Error('Failed to fetch data');
      
      const dashJson = await dashRes.json();
      const trendJson = await trendRes.json();
      
      setData(dashJson);
      setTrending(trendJson.trending || []);
    } catch (e) {
      console.error("Failed to load analytics", e);
      if (!isAutoRefresh) setError(true);
    } finally {
      if (!isAutoRefresh) setLoading(false);
      else setRefreshing(false);
    }
  }, []);

  useEffect(() => {
    fetchData();
    
    // Real-time update capability: Poll every 30 seconds
    const interval = setInterval(() => {
      fetchData(true);
    }, 30000);
    
    return () => clearInterval(interval);
  }, [fetchData]);

  if (loading) {
    return (
      <div className="min-h-screen bg-background flex flex-col">
        <Navbar />
        <div className="flex-1 flex items-center justify-center">
          <div className="flex flex-col items-center gap-4">
            <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-primary"></div>
            <p className="text-sm text-muted-foreground animate-pulse">Loading real-time analytics...</p>
          </div>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="min-h-screen bg-background flex flex-col">
        <Navbar />
        <div className="flex-1 flex items-center justify-center p-4">
          <div className="bg-card p-8 rounded-2xl shadow-lg border border-red-500/20 max-w-md w-full text-center">
            <AlertCircle className="w-12 h-12 text-red-500 mx-auto mb-4" />
            <h2 className="text-xl font-bold text-foreground mb-2">Failed to load analytics</h2>
            <p className="text-sm text-muted-foreground mb-6">There was an issue connecting to the analytics service. Please check your connection and try again.</p>
            <button onClick={() => fetchData()} className="w-full py-2.5 bg-primary hover:opacity-90 text-primary-foreground rounded-lg inline-flex items-center justify-center transition-colors font-medium">
              <RefreshCw className="w-4 h-4 mr-2" /> Try Again
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-background flex flex-col">
      <Navbar />
      <main className="flex-1 max-w-7xl w-full mx-auto px-4 sm:px-6 lg:px-8 py-8 space-y-8">
        <header className="flex flex-col md:flex-row md:items-center justify-between gap-4">
          <div>
            <div className="flex items-center gap-2 mb-1">
              <Activity className="w-5 h-5 text-primary" />
              <span className="text-xs font-bold text-primary uppercase tracking-wider">Live Metrics</span>
            </div>
            <h1 className="text-4xl font-black text-foreground tracking-tight">Ecosystem Insights</h1>
            <p className="text-muted-foreground mt-1.5 text-sm">Monitoring deployment trends, category shifts, and top contributors.</p>
          </div>
          <div className="flex items-center gap-3">
             {refreshing && (
                <span className="text-[10px] text-muted-foreground flex items-center gap-1 animate-pulse">
                  <RefreshCw className="w-3 h-3 animate-spin" /> Updating...
                </span>
             )}
             <button 
               onClick={() => fetchData()} 
               disabled={refreshing}
               className="p-2 border border-border rounded-lg bg-card hover:bg-muted transition-colors"
             >
               <RefreshCw className={`w-4 h-4 ${refreshing ? 'animate-spin' : ''}`} />
             </button>
          </div>
        </header>

        {/* Top Stats Overview */}
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
           <div className="bg-card border border-border rounded-2xl p-6 shadow-sm">
              <p className="text-xs font-bold text-muted-foreground uppercase tracking-widest mb-1">Total Views</p>
              <p className="text-3xl font-black text-foreground">
                {data?.category_distribution?.reduce((acc: number, cur: any) => acc + cur.total_views, 0).toLocaleString() || 0}
              </p>
           </div>
           <div className="bg-card border border-border rounded-2xl p-6 shadow-sm">
              <p className="text-xs font-bold text-muted-foreground uppercase tracking-widest mb-1">Active Contracts</p>
              <p className="text-3xl font-black text-foreground">
                {data?.category_distribution?.reduce((acc: number, cur: any) => acc + cur.contract_count, 0).toLocaleString() || 0}
              </p>
           </div>
           <div className="bg-card border border-border rounded-2xl p-6 shadow-sm">
              <p className="text-xs font-bold text-muted-foreground uppercase tracking-widest mb-1">Verified Gate</p>
              <p className="text-3xl font-black text-foreground">
                {data?.network_usage?.reduce((acc: number, cur: any) => acc + cur.verified_count, 0).toLocaleString() || 0}
              </p>
           </div>
           <div className="bg-card border border-border rounded-2xl p-6 shadow-sm bg-gradient-to-br from-card to-primary/5">
              <p className="text-xs font-bold text-primary uppercase tracking-widest mb-1">Top Language</p>
              <p className="text-3xl font-black text-foreground">Rust/WASM</p>
           </div>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-12 gap-8">
            {/* Primary Charts */}
            <div className="col-span-12 lg:col-span-8 space-y-8">
                <div className="bg-card border border-border rounded-2xl shadow-sm overflow-hidden p-6">
                    <div className="flex items-center gap-2 mb-6">
                        <Clock className="w-5 h-5 text-primary" />
                        <h3 className="font-bold text-lg">Deployment Trends</h3>
                    </div>
                    <div className="h-[300px]">
                        <DeploymentTrendGraph data={data?.deployment_trends || []} />
                    </div>
                </div>

                <div className="bg-card border border-border rounded-2xl shadow-sm overflow-hidden">
                    <div className="p-6 border-b border-border flex items-center justify-between">
                        <div className="flex items-center gap-2">
                            <Flame className="w-5 h-5 text-orange-500" />
                            <div>
                                <h3 className="font-bold">Trending Contracts</h3>
                                <p className="text-xs text-muted-foreground">Most active in the last 7 days</p>
                            </div>
                        </div>
                    </div>
                    <div className="p-0">
                        <TrendingContractsTable data={trending} />
                    </div>
                </div>
            </div>

            {/* Sidebar Stats */}
            <div className="col-span-12 lg:col-span-4 space-y-8">
                <div className="bg-card border border-border rounded-2xl shadow-sm p-6">
                    <div className="flex items-center gap-2 mb-6">
                        <PieChart className="w-5 h-5 text-pink-500" />
                        <h3 className="font-bold">By Category</h3>
                    </div>
                    <CategoryDistributionPie data={data?.category_distribution?.map((i: any) => ({ category: i.category || 'Other', count: i.contract_count })) || []} />
                </div>

                <div className="bg-card border border-border rounded-2xl shadow-sm p-6">
                    <div className="flex items-center gap-2 mb-6">
                        <BarChart3 className="w-5 h-5 text-blue-500" />
                        <h3 className="font-bold">Network Usage</h3>
                    </div>
                    <NetworkUsageBarChart data={data?.network_usage?.map((i: any) => ({ network: i.network, contract_count: i.contract_count })) || []} />
                </div>

                <div className="bg-card border border-border rounded-2xl shadow-sm p-6">
                    <div className="flex items-center gap-2 mb-6">
                        <Users className="w-5 h-5 text-green-500" />
                        <h3 className="font-bold">Top Publishers</h3>
                    </div>
                    <TopPublishersList data={data?.top_publishers || []} />
                </div>

                <div className="bg-card border border-border rounded-2xl shadow-sm p-6">
                    <div className="flex items-center gap-2 mb-6">
                        <Clock className="w-5 h-5 text-purple-500" />
                        <h3 className="font-bold">Recent Additions</h3>
                    </div>
                    <RecentAdditionsTimeline data={data?.recent_additions || []} />
                </div>
            </div>
        </div>
      </main>
    </div>
  );
}
