'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useSearchParams } from 'next/navigation';
import { api, type Contract } from '@/lib/api';
import { toComparableContract, getMetricValue, toneForMetricCell, type ComparableContract, type ComparisonMetricKey, type CellTone } from '@/utils/comparison';
import { parseContractIdsFromSearch, replaceUrlContractIds } from '@/utils/urlState';

type ComparisonMetric = {
  key: ComparisonMetricKey;
  label: string;
  getDisplayValue: (c: ComparableContract) => string;
  getRawValue: (c: ComparableContract) => number | boolean;
};

type MetricTones = Record<ComparisonMetricKey, Record<string, CellTone>>;

function uniq(ids: string[]) {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const id of ids) {
    if (!seen.has(id)) {
      seen.add(id);
      out.push(id);
    }
  }
  return out;
}

export function useComparison() {
  const searchParams = useSearchParams();

  const initialSelectedIds = useMemo(() => {
    const ids = parseContractIdsFromSearch(searchParams?.toString() ?? '');
    return uniq(ids).slice(0, 4);
  }, [searchParams]);

  const [selectedIds, setSelectedIds] = useState<string[]>(() => initialSelectedIds);
  const [selectionError, setSelectionError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [baselineId, setBaselineId] = useState<string | null>(() => initialSelectedIds[0] ?? null);

  useEffect(() => {
    replaceUrlContractIds(selectedIds);
  }, [selectedIds]);

  const contractsSearchQuery = useQuery({
    queryKey: ['compare', 'contracts-search', searchQuery],
    queryFn: async () => {
      const res = await api.getContracts({
        query: searchQuery || undefined,
        page: 1,
        page_size: 25,
        sort_by: searchQuery ? 'relevance' : 'created_at',
        sort_order: 'desc',
      });
      return res.items;
    },
    staleTime: 30_000,
  });

  const selectedContractsQuery = useQuery({
    queryKey: ['compare', 'selected-contracts', selectedIds],
    queryFn: async () => {
      const ids = selectedIds.slice(0, 4);
      if (ids.length === 0) return [];
      const results = await Promise.all(
        ids.map(async (id) => {
          const c = await api.getContract(id);
          return c;
        }),
      );
      return results;
    },
    enabled: selectedIds.length > 0,
    staleTime: 30_000,
  });

  const selectedContracts = useMemo<ComparableContract[]>(() => {
    const contracts = selectedContractsQuery.data ?? [];
    const byId = new Map<string, ComparableContract>();
    for (const c of contracts) {
      byId.set(c.id, toComparableContract(c));
    }
    return selectedIds
      .map((id) => byId.get(id))
      .filter((c): c is ComparableContract => Boolean(c));
  }, [selectedContractsQuery.data, selectedIds]);

  const effectiveBaselineId = useMemo(() => {
    if (!baselineId) return selectedContracts[0]?.id ?? null;
    return selectedContracts.some((c) => c.id === baselineId) ? baselineId : selectedContracts[0]?.id ?? null;
  }, [baselineId, selectedContracts]);

  const metrics = useMemo<ComparisonMetric[]>(
    () => [
      {
        key: 'abi_method_count',
        label: 'ABI method count',
        getDisplayValue: (c) => String(c.abiMethods.length),
        getRawValue: (c) => c.abiMethods.length,
      },
      {
        key: 'gas_estimate',
        label: 'Gas estimate',
        getDisplayValue: (c) => `${c.gasEstimate.toLocaleString()} units`,
        getRawValue: (c) => c.gasEstimate,
      },
      {
        key: 'deployment_count',
        label: 'Deployment count',
        getDisplayValue: (c) => c.deploymentCount.toLocaleString(),
        getRawValue: (c) => c.deploymentCount,
      },
      {
        key: 'verification_status',
        label: 'Verification status',
        getDisplayValue: (c) => (c.isVerified ? 'Verified' : 'Unverified'),
        getRawValue: (c) => c.isVerified,
      },
    ],
    [],
  );

  const metricTones = useMemo<MetricTones>(() => {
    const tones = {} as MetricTones;
    for (const metric of metrics) {
      const values = selectedContracts.map((c) => getMetricValue(c, metric.key));
      tones[metric.key] = {};
      for (const c of selectedContracts) {
        tones[metric.key][c.id] = toneForMetricCell(metric.key, getMetricValue(c, metric.key), values);
      }
    }
    return tones;
  }, [metrics, selectedContracts]);

  const addContract = useCallback(
    (contract: Pick<Contract, 'id'>) => {
      setSelectionError(null);

      setSelectedIds((prev) => {
        if (prev.includes(contract.id)) {
          setSelectionError('That contract is already selected.');
          return prev;
        }
        if (prev.length >= 4) {
          setSelectionError('You can compare up to 4 contracts.');
          return prev;
        }
        const next = [...prev, contract.id];
        if (!baselineId) setBaselineId(next[0] ?? null);
        return next;
      });
    },
    [baselineId],
  );

  const removeContract = useCallback((contractId: string) => {
    setSelectionError(null);
    setSelectedIds((prev) => prev.filter((id) => id !== contractId));
  }, []);

  const replaceSelectedIds = useCallback((ids: string[]) => {
    const next = uniq(ids).slice(0, 4);
    setSelectionError(null);
    setSelectedIds(next);
    setBaselineId(next[0] ?? null);
  }, []);

  const selectionCountValid = selectedIds.length >= 2 && selectedIds.length <= 4;
  const selectionCountError =
    selectedIds.length > 4
      ? 'You can compare up to 4 contracts.'
      : selectedIds.length > 0 && selectedIds.length < 2
        ? 'Select at least 2 contracts to compare.'
        : null;

  return {
    searchQuery,
    setSearchQuery,
    contractsSearch: {
      items: contractsSearchQuery.data ?? [],
      isLoading: contractsSearchQuery.isLoading,
    },
    selectedIds,
    selectedContracts,
    selectedContractsQuery: {
      isLoading: selectedContractsQuery.isLoading,
      isError: selectedContractsQuery.isError,
    },
    selectionError,
    selectionCountError,
    selectionCountValid,
    addContract,
    removeContract,
    replaceSelectedIds,
    metrics,
    metricTones,
    baselineId: effectiveBaselineId,
    setBaselineId,
  };
}
