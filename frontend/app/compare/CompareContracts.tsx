'use client';

import { useMemo, useState } from 'react';
import dynamic from 'next/dynamic';
import { BarChart2, Code2, Download, Link2 } from 'lucide-react';
import { useComparison } from '@/hooks/useComparison';
import ContractSelector from '@/components/comparison/ContractSelector';
import ComparisonTable from '@/components/comparison/ComparisonTable';
import MobileComparisonCard from '@/components/comparison/MobileComparisonCard';
import { useCopy } from '@/hooks/useCopy';
import { exportComparisonToCsv, exportComparisonToPdf } from '@/utils/export';

export default function CompareContracts() {
  const [viewMode, setViewMode] = useState<'table' | 'diff'>('table');
  const {
    searchQuery,
    setSearchQuery,
    contractsSearch,
    selectedContracts,
    selectionError,
    selectionCountError,
    selectionCountValid,
    addContract,
    removeContract,
    metrics,
    metricTones,
    baselineId,
    setBaselineId,
  } = useComparison();

  const DiffViewer = useMemo(
    () =>
      dynamic(() => import('@/components/comparison/DiffViewer'), {
        ssr: false,
        loading: () => (
          <div className="rounded-2xl border border-border bg-card p-6">
            <div className="text-sm font-semibold text-foreground">Loading diff viewer…</div>
          </div>
        ),
      }),
    [],
  );

  const selectedChips = useMemo(
    () => selectedContracts.map((c) => ({ id: c.id, name: c.name })),
    [selectedContracts],
  );

  const canExport = selectedContracts.length >= 2;
  const { copy, copied, isCopying } = useCopy();

  const exportRowsForPdf = useMemo(
    () =>
      metrics.map((m) => ({
        label: m.label,
        values: selectedContracts.map((c) => m.getDisplayValue(c)),
      })),
    [metrics, selectedContracts],
  );

  return (
    <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-10">
      <div className="flex flex-col gap-6">
        <div className="rounded-2xl border border-border bg-card p-6">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
            <div>
              <h1 className="text-2xl font-bold text-foreground">Compare Contracts</h1>
              <p className="mt-2 text-sm text-muted-foreground">
                Select 2–4 contracts, compare key attributes side-by-side, and inspect ABI/source diffs.
              </p>
            </div>
            <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => setViewMode('table')}
                  className={`inline-flex items-center gap-2 rounded-xl border px-3 py-2 text-sm font-semibold transition-colors ${
                    viewMode === 'table'
                      ? 'border-primary/30 bg-primary/10 text-primary'
                      : 'border-border bg-background text-muted-foreground hover:text-foreground hover:bg-accent'
                  }`}
                >
                  <BarChart2 className="h-4 w-4" />
                  Table
                </button>
                <button
                  type="button"
                  onClick={() => setViewMode('diff')}
                  className={`inline-flex items-center gap-2 rounded-xl border px-3 py-2 text-sm font-semibold transition-colors ${
                    viewMode === 'diff'
                      ? 'border-primary/30 bg-primary/10 text-primary'
                      : 'border-border bg-background text-muted-foreground hover:text-foreground hover:bg-accent'
                  }`}
                >
                  <Code2 className="h-4 w-4" />
                  Diff
                </button>
              </div>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  disabled={!canExport}
                  onClick={() =>
                    exportComparisonToCsv(
                      selectedContracts,
                      metrics.map((m) => ({
                        key: m.key,
                        label: m.label,
                        getValue: (c) =>
                          m.key === 'verification_status' ? m.getDisplayValue(c) : m.getRawValue(c),
                      })),
                    )
                  }
                  className={`inline-flex items-center gap-2 rounded-xl border px-3 py-2 text-sm font-semibold transition-colors ${
                    canExport
                      ? 'border-border bg-background text-muted-foreground hover:text-foreground hover:bg-accent'
                      : 'border-border bg-muted text-muted-foreground cursor-not-allowed opacity-70'
                  }`}
                >
                  <Download className="h-4 w-4" />
                  CSV
                </button>
                <button
                  type="button"
                  disabled={!canExport}
                  onClick={() => exportComparisonToPdf(selectedContracts, exportRowsForPdf)}
                  className={`inline-flex items-center gap-2 rounded-xl border px-3 py-2 text-sm font-semibold transition-colors ${
                    canExport
                      ? 'border-border bg-background text-muted-foreground hover:text-foreground hover:bg-accent'
                      : 'border-border bg-muted text-muted-foreground cursor-not-allowed opacity-70'
                  }`}
                >
                  <Download className="h-4 w-4" />
                  PDF
                </button>
                <button
                  type="button"
                  onClick={() => copy(window.location.href, { successEventName: 'comparison_link_copied' })}
                  disabled={isCopying}
                  className="inline-flex items-center gap-2 rounded-xl border border-border bg-background px-3 py-2 text-sm font-semibold text-muted-foreground hover:text-foreground hover:bg-accent transition-colors disabled:opacity-60"
                >
                  <Link2 className="h-4 w-4" />
                  {copied ? 'Copied' : 'Share'}
                </button>
              </div>
            </div>
          </div>
        </div>

        <ContractSelector
          available={contractsSearch.items}
          isLoading={contractsSearch.isLoading}
          searchQuery={searchQuery}
          onSearchQueryChange={setSearchQuery}
          selected={selectedChips}
          onAdd={addContract}
          onRemove={removeContract}
          selectionError={selectionError}
          selectionCountError={selectionCountError}
        />

        {viewMode === 'table' ? (
          <div className="flex flex-col gap-4">
            {!selectionCountValid && (
              <div className="rounded-2xl border border-border bg-card p-6">
                <div className="text-sm font-semibold text-foreground">Add at least 2 contracts to compare.</div>
                <div className="mt-1 text-xs text-muted-foreground">Use the selector above to pick contracts.</div>
              </div>
            )}

            {selectedContracts.length > 0 && (
              <>
                <ComparisonTable contracts={selectedContracts} metrics={metrics} tones={metricTones} />
                <div className="lg:hidden grid grid-cols-1 gap-4">
                  {selectedContracts.map((c) => (
                    <MobileComparisonCard key={c.id} contract={c} metrics={metrics} tones={metricTones} />
                  ))}
                </div>
              </>
            )}
          </div>
        ) : (
          <DiffViewer contracts={selectedContracts} baselineId={baselineId} onBaselineIdChange={setBaselineId} />
        )}
      </div>
    </main>
  );
}

