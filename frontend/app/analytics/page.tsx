'use client';

import React, { useState, useEffect } from 'react';
import Navbar from '@/components/Navbar';
import TrendingContractsTable from '@/components/analytics/TrendingContractsTable';
import CategoryDistributionPie from '@/components/analytics/CategoryDistributionPie';
import DeploymentTrendGraph from '@/components/analytics/DeploymentTrendGraph';
import NetworkUsageStats from '@/components/analytics/NetworkUsageStats';
import RecentAdditionsTimeline from '@/components/analytics/RecentAdditionsTimeline';
import DeploymentTimeline from '@/components/analytics/DeploymentTimeline';
import { AlertCircle, RefreshCw } from 'lucide-react';

export default function AnalyticsDashboard() {
  const [data, setData] = useState<Record<string, unknown> | null>(null);
  const [trending, setTrending] = useState<Record<string, unknown>[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);

  const fetchData = async () => {
    setLoading(true);
    setError(false);
    try {
      const [dashRes, trendRes] = await Promise.all([
        fetch(process.env.NEXT_PUBLIC_API_URL ? `${process.env.NEXT_PUBLIC_API_URL}/api/analytics/dashboard` : '/api/analytics/dashboard'),
        fetch(process.env.NEXT_PUBLIC_API_URL ? `${process.env.NEXT_PUBLIC_API_URL}/api/contracts/trending?limit=10` : '/api/contracts/trending?limit=10')
      ]);
      const dashJson = await dashRes.json();
      const trendJson = await trendRes.json();
      setData(dashJson);
      setTrending(trendJson);
    } catch (e) {
      console.error("Failed to load analytics", e);
      setError(true);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchData();
  }, []);

  if (error) {
    return (
      <div className="min-h-screen bg-background flex flex-col">
        <Navbar />
        <div className="flex-1 flex items-center justify-center p-4">
          <div className="bg-card p-8 rounded-2xl shadow-lg border border-red-500/20 max-w-md w-full text-center">
            <AlertCircle className="w-12 h-12 text-red-500 mx-auto mb-4" />
            <h2 className="text-xl font-bold text-foreground mb-2">Failed to load analytics</h2>
            <button onClick={fetchData} className="px-4 py-2 bg-primary hover:opacity-90 text-primary-foreground rounded-lg inline-flex items-center transition-colors">
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
      <main className="flex-1 max-w-7xl w-full mx-auto px-4 sm:px-6 lg:px-8 py-8 space-y-6">
        <header className="flex flex-col md:flex-row md:items-end justify-between gap-4">
          <div>
            <h1 className="text-3xl font-bold text-foreground tracking-tight">Analytics Dashboard</h1>
            <p className="text-muted-foreground mt-1.5 text-sm">Real-time insights on contract popularity and registry statistics</p>
          </div>
        </header>

        {loading ? (
           <div className="h-[60vh] flex items-center justify-center">
              <div className="animate-spin rounded-full h-10 w-10 border-b-2 border-primary"></div>
           </div>
        ) : (
          <div className="space-y-6 animate-in fade-in duration-500">
            <div className="w-full">
                {/* eslint-disable-next-line @typescript-eslint/no-explicit-any */}
                <DeploymentTimeline initialData={data?.recent_additions as any || []} />
            </div>

            <div className="grid grid-cols-1 md:grid-cols-12 gap-6">
                <div className="col-span-12 lg:col-span-8">
                    {/* eslint-disable-next-line @typescript-eslint/no-explicit-any */}
                    <NetworkUsageStats data={(data?.network_usage as any[]) || []} />
                </div>
                <div className="col-span-12 lg:col-span-4 bg-card border border-border rounded-xl shadow-sm p-5 h-[350px]">
                    <h3 className="text-sm font-semibold text-foreground mb-4">Category Distribution</h3>
                    {/* eslint-disable-next-line @typescript-eslint/no-explicit-any */}
                    <CategoryDistributionPie data={(data?.category_distribution as any[]) || []} />
                </div>
            </div>

            <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
                <div className="bg-card border border-border rounded-xl shadow-sm p-5 min-h-[350px]">
                    <h3 className="text-sm font-semibold text-foreground mb-4">Deployment Trends (30d)</h3>
                    {/* eslint-disable-next-line @typescript-eslint/no-explicit-any */}
                    <DeploymentTrendGraph data={(data?.deployment_trends as any[]) || []} />
                </div>
                <div className="bg-card border border-border rounded-xl shadow-sm p-5 min-h-[350px]">
                    <h3 className="text-sm font-semibold text-foreground mb-4">Recent Additions</h3>
                    {/* eslint-disable-next-line @typescript-eslint/no-explicit-any */}
                    <RecentAdditionsTimeline data={(data?.recent_additions as any[]) || []} />
                </div>
            </div>

            <div className="bg-card border border-border rounded-xl shadow-sm p-0 overflow-hidden">
                <div className="p-5 border-b border-border">
                    <h3 className="text-sm font-semibold text-foreground">Trending Contracts</h3>
                    <p className="text-xs text-muted-foreground mt-1">Top 10 most interacted contracts over the past week</p>
                </div>
                <TrendingContractsTable data={trending} />
            </div>
          </div>
        )}
      </main>
    </div>
  );
}
