"use client";

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import type { PerformanceTrendPoint } from "@/lib/api";
import {
  AlertTriangle,
  Gauge,
  TimerReset,
  TrendingUp,
} from "lucide-react";
import {
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

interface PerformancePanelProps {
  contractId: string;
}

function formatDelta(value?: number | null): string {
  if (value == null || Number.isNaN(value)) return "No baseline";
  const prefix = value > 0 ? "+" : "";
  return `${prefix}${value.toFixed(1)}%`;
}

function trendChartData(points: PerformanceTrendPoint[]) {
  const grouped = new Map<
    string,
    {
      date: string;
      execution_time_total: number;
      gas_used_total: number;
      count: number;
    }
  >();

  for (const point of [...points].reverse()) {
    const key = point.bucket_start.slice(0, 10);
    const current = grouped.get(key) ?? {
      date: point.bucket_start.slice(5, 10),
      execution_time_total: 0,
      gas_used_total: 0,
      count: 0,
    };

    current.execution_time_total += point.avg_execution_time_ms;
    current.gas_used_total += point.avg_gas_used;
    current.count += 1;
    grouped.set(key, current);
  }

  return Array.from(grouped.values()).map((entry) => ({
    date: entry.date,
    execution_time_ms: entry.execution_time_total / entry.count,
    gas_used: entry.gas_used_total / entry.count,
  }));
}

export default function PerformancePanel({ contractId }: PerformancePanelProps) {
  const { data, isLoading, error } = useQuery({
    queryKey: ["contract-performance", contractId],
    queryFn: () => api.getContractPerformance(contractId),
  });

  const chartData = useMemo(() => trendChartData(data?.trends ?? []), [data?.trends]);

  return (
    <section className="space-y-6">
      <div className="flex items-center gap-3">
        <Gauge className="w-6 h-6 text-primary" />
        <div>
          <h2 className="text-2xl font-bold text-foreground">Performance Benchmarks</h2>
          <p className="text-sm text-muted-foreground">
            Gas usage, execution time, version regressions, and peer comparison.
          </p>
        </div>
      </div>

      {isLoading && (
        <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
          {[1, 2, 3].map((item) => (
            <div
              key={item}
              className="h-36 rounded-2xl border border-border bg-card animate-pulse"
            />
          ))}
        </div>
      )}

      {error && (
        <div className="rounded-2xl border border-red-500/20 bg-red-500/5 p-5 text-sm text-red-600 dark:text-red-400">
          Unable to load contract performance metrics right now.
        </div>
      )}

      {data && !isLoading && !error && (
        <>
          <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
            {data.latest_benchmarks.length === 0 ? (
              <div className="rounded-2xl border border-border bg-card p-6 text-sm text-muted-foreground md:col-span-2 xl:col-span-3">
                No benchmarks recorded yet for this contract.
              </div>
            ) : (
              data.latest_benchmarks.map((benchmark) => {
                const matchingExecution = data.metric_snapshots.find(
                  (snapshot) =>
                    snapshot.metric_type === "execution_time" &&
                    snapshot.benchmark_name === benchmark.benchmark_name,
                );
                const matchingGas = data.metric_snapshots.find(
                  (snapshot) =>
                    snapshot.metric_type === "gas_consumption" &&
                    snapshot.benchmark_name === benchmark.benchmark_name,
                );

                return (
                  <div
                    key={benchmark.id}
                    className="rounded-2xl border border-border bg-card p-5 space-y-4"
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div>
                        <div className="text-sm text-muted-foreground">Benchmark</div>
                        <h3 className="text-lg font-semibold text-foreground">
                          {benchmark.benchmark_name}
                        </h3>
                      </div>
                      <span className="rounded-full bg-primary/10 px-3 py-1 text-xs font-medium text-primary">
                        {benchmark.version ?? "latest"}
                      </span>
                    </div>

                    <div className="grid grid-cols-2 gap-3 text-sm">
                      <div className="rounded-xl bg-accent/60 p-3">
                        <div className="text-muted-foreground flex items-center gap-2">
                          <TimerReset className="w-4 h-4" />
                          Exec time
                        </div>
                        <div className="mt-1 text-xl font-semibold text-foreground">
                          {benchmark.execution_time_ms.toFixed(2)} ms
                        </div>
                        <div className="text-xs text-muted-foreground">
                          {formatDelta(matchingExecution?.change_percent)}
                        </div>
                      </div>
                      <div className="rounded-xl bg-accent/60 p-3">
                        <div className="text-muted-foreground flex items-center gap-2">
                          <TrendingUp className="w-4 h-4" />
                          Gas used
                        </div>
                        <div className="mt-1 text-xl font-semibold text-foreground">
                          {benchmark.gas_used.toLocaleString()}
                        </div>
                        <div className="text-xs text-muted-foreground">
                          {formatDelta(matchingGas?.change_percent)}
                        </div>
                      </div>
                    </div>

                    <div className="text-xs text-muted-foreground">
                      Source: {benchmark.source} • Samples: {benchmark.sample_size} •{" "}
                      {new Date(benchmark.recorded_at).toLocaleDateString()}
                    </div>
                  </div>
                );
              })
            )}
          </div>

          <div className="grid grid-cols-1 xl:grid-cols-3 gap-6">
            <div className="xl:col-span-2 rounded-2xl border border-border bg-card p-6">
              <h3 className="text-lg font-semibold text-foreground mb-4">
                Trend over time
              </h3>
              <div className="h-72">
                {chartData.length === 0 ? (
                  <div className="h-full flex items-center justify-center text-sm text-muted-foreground">
                    Not enough benchmark history yet to show a trend line.
                  </div>
                ) : (
                  <ResponsiveContainer width="100%" height="100%">
                    <LineChart data={chartData} margin={{ top: 8, right: 16, left: 0, bottom: 0 }}>
                      <CartesianGrid strokeDasharray="3 3" className="stroke-gray-200 dark:stroke-gray-700" />
                      <XAxis dataKey="date" tick={{ fontSize: 11, fill: "currentColor" }} />
                      <YAxis yAxisId="left" tick={{ fontSize: 11, fill: "currentColor" }} />
                      <YAxis yAxisId="right" orientation="right" tick={{ fontSize: 11, fill: "currentColor" }} />
                      <Tooltip />
                      <Legend />
                      <Line
                        yAxisId="left"
                        type="monotone"
                        dataKey="execution_time_ms"
                        stroke="#2563eb"
                        strokeWidth={2}
                        dot={false}
                        name="Exec time (ms)"
                      />
                      <Line
                        yAxisId="right"
                        type="monotone"
                        dataKey="gas_used"
                        stroke="#f97316"
                        strokeWidth={2}
                        dot={false}
                        name="Gas used"
                      />
                    </LineChart>
                  </ResponsiveContainer>
                )}
              </div>
            </div>

            <div className="rounded-2xl border border-border bg-card p-6">
              <h3 className="text-lg font-semibold text-foreground mb-4">
                Regression Alerts
              </h3>
              <div className="space-y-3">
                {data.regressions.length === 0 && data.unresolved_alerts.length === 0 ? (
                  <p className="text-sm text-muted-foreground">
                    No active regressions detected.
                  </p>
                ) : (
                  <>
                    {data.regressions.map((regression) => (
                      <div
                        key={`${regression.benchmark_name}-${regression.detected_at}`}
                        className="rounded-xl border border-amber-500/20 bg-amber-500/5 p-4"
                      >
                        <div className="flex items-start gap-2 text-sm">
                          <AlertTriangle className="w-4 h-4 mt-0.5 text-amber-500" />
                          <div>
                            <div className="font-medium text-foreground">
                              {regression.benchmark_name}
                            </div>
                            <div className="text-muted-foreground">
                              Execution {formatDelta(regression.execution_time_regression_percent)} • Gas{" "}
                              {formatDelta(regression.gas_regression_percent)}
                            </div>
                            <div className="text-xs text-muted-foreground mt-1">
                              {regression.previous_version ?? "previous"} to{" "}
                              {regression.current_version ?? "current"}
                            </div>
                          </div>
                        </div>
                      </div>
                    ))}
                    {data.unresolved_alerts.slice(0, 3).map((alert) => (
                      <div
                        key={alert.id}
                        className="rounded-xl border border-border bg-accent/40 p-4 text-sm"
                      >
                        <div className="font-medium text-foreground">{alert.severity}</div>
                        <div className="text-muted-foreground">
                          {alert.message ?? "Performance threshold exceeded"}
                        </div>
                      </div>
                    ))}
                  </>
                )}
              </div>
            </div>
          </div>

          <div className="rounded-2xl border border-border bg-card p-6">
            <h3 className="text-lg font-semibold text-foreground mb-4">
              Similar contract comparison
            </h3>
            {data.comparisons.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                No peer benchmarks found in the same contract category yet.
              </p>
            ) : (
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="border-b border-border text-left text-muted-foreground">
                      <th className="pb-3 font-medium">Contract</th>
                      <th className="pb-3 font-medium">Benchmark</th>
                      <th className="pb-3 font-medium">Avg exec</th>
                      <th className="pb-3 font-medium">Avg gas</th>
                      <th className="pb-3 font-medium">Samples</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.comparisons.map((item) => (
                      <tr key={`${item.contract_id}-${item.benchmark_name}`} className="border-b border-border/60">
                        <td className="py-3 font-medium text-foreground">{item.contract_name}</td>
                        <td className="py-3 text-muted-foreground">{item.benchmark_name}</td>
                        <td className="py-3 text-foreground">{item.avg_execution_time_ms.toFixed(2)} ms</td>
                        <td className="py-3 text-foreground">{item.avg_gas_used.toFixed(0)}</td>
                        <td className="py-3 text-muted-foreground">{item.sample_count}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </div>
        </>
      )}
    </section>
  );
}
