'use client';

import React, { useState, useEffect, useMemo, useRef } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, ContractSearchParams, Contract } from '@/lib/api';
import { useDebouncedValue } from '@/hooks/useDebouncedValue';
import ContractCard from '@/components/ContractCard';
import ContractCardSkeleton from '@/components/ContractCardSkeleton';
import { ActiveFilters } from '@/components/contracts/ActiveFilters';
import { FilterPanel } from '@/components/contracts/FilterPanel';
import { ResultsCount } from '@/components/contracts/ResultsCount';
import { SortDropdown, SortBy } from '@/components/contracts/SortDropdown';
import TagAutocomplete from '@/components/tags/TagAutocomplete';
import { Filter, Package, SlidersHorizontal, X, Search, Sparkles, CheckCircle, Users } from 'lucide-react';
import { usePathname, useRouter, useSearchParams } from 'next/navigation';
import { useAnalytics } from '@/hooks/useAnalytics';

const DEFAULT_PAGE_SIZE = 12;
const CATEGORY_OPTIONS = [
  'DeFi',
  'NFT',
  'Governance',
  'Infrastructure',
  'Payment',
  'Identity',
  'Gaming',
  'Social',
];
const LANGUAGE_OPTIONS = [
  'Rust',
  'TypeScript',
  'JavaScript',
  'AssemblyScript',
  'Move',
];

function parseCsvOrMulti(values: string[]) {
  return values
    .flatMap((value) => value.split(','))
    .map((value) => value.trim())
    .filter(Boolean);
}

function removeOne<T>(values: T[], value: T) {
  return values.filter((current) => current !== value);
}

function toggleOne<T>(values: T[], value: T) {
  return values.includes(value)
    ? values.filter((current) => current !== value)
    : [...values, value];
}

function getPaginationRange(
  currentPage: number,
  totalPages: number,
): Array<number | 'ellipsis'> {
  if (totalPages <= 0) return [];
  if (totalPages <= 5) {
    return Array.from({ length: totalPages }, (_, index) => index + 1);
  }

  const safeCurrentPage = Math.min(Math.max(1, currentPage), totalPages);
  const isNearStart = safeCurrentPage <= 2;
  const isNearEnd = safeCurrentPage >= totalPages - 1;

  const windowStart = isNearStart ? 2 : isNearEnd ? totalPages - 2 : safeCurrentPage - 1;
  const windowEnd = isNearStart ? 3 : isNearEnd ? totalPages - 1 : safeCurrentPage + 1;

  const basePages = [
    1,
    ...Array.from(
      { length: Math.max(0, windowEnd - windowStart + 1) },
      (_, index) => windowStart + index,
    ),
    totalPages,
  ].filter((page) => page >= 1 && page <= totalPages);

  const uniqueSortedPages = Array.from(new Set(basePages)).sort((a, b) => a - b);
  const range: Array<number | 'ellipsis'> = [];

  for (let index = 0; index < uniqueSortedPages.length; index += 1) {
    const page = uniqueSortedPages[index];
    const previous = range[range.length - 1];

    if (typeof previous !== 'number') {
      range.push(page);
      continue;
    }

    const diff = page - previous;
    if (diff === 2) {
      range.push(previous + 1, page);
      continue;
    }
    if (diff > 2) {
      range.push('ellipsis', page);
      continue;
    }

    range.push(page);
  }

  return range;
}

type ContractsUiFilters = {
  query: string;
  categories: string[];
  languages: string[];
  tags: string[];
  author: string;
  networks: NonNullable<ContractSearchParams['network']>[];
  verified_only: boolean;
  sort_by: SortBy;
  sort_order: 'asc' | 'desc';
  page: number;
  page_size: number;
};

type ContractsResponse = Awaited<ReturnType<typeof api.getContracts>>;

const EMPTY_CONTRACTS_RESPONSE: ContractsResponse = {
  items: [],
  total: 0,
  page: 1,
  page_size: DEFAULT_PAGE_SIZE,
  total_pages: 1,
};

function getInitialFilters(searchParams: URLSearchParams): ContractsUiFilters {
  const query = searchParams.get('query') || searchParams.get('q') || '';
  const categories = parseCsvOrMulti(searchParams.getAll('category'));
  const languages = parseCsvOrMulti(searchParams.getAll('language'));
  const tags = parseCsvOrMulti(searchParams.getAll('tag'));
  const networks = parseCsvOrMulti(searchParams.getAll('network')).filter(
    (network): network is NonNullable<ContractSearchParams['network']> =>
      network === 'mainnet' || network === 'testnet' || network === 'futurenet',
  );

  const sortBy = searchParams.get('sort_by') as SortBy;
  const sortOrder = searchParams.get('sort_order') as 'asc' | 'desc';
  const parsedPage = Number(searchParams.get('page') || '1');

  const validSortBys: SortBy[] = ['name', 'created_at', 'updated_at', 'popularity', 'deployments', 'interactions', 'relevance', 'downloads'];

  return {
    query,
    categories,
    languages,
    tags,
    author: searchParams.get('author') || '',
    networks,
    verified_only: searchParams.get('verified_only') === 'true',
    sort_by: validSortBys.includes(sortBy) ? sortBy : (query ? 'relevance' : 'created_at'),
    sort_order: sortOrder === 'asc' || sortOrder === 'desc' ? sortOrder : 'desc',
    page: Number.isFinite(parsedPage) && parsedPage > 0 ? parsedPage : 1,
    page_size: DEFAULT_PAGE_SIZE,
  };
}

export function ContractsContent() {
  const router = useRouter();
  const pathname = usePathname() ?? '/contracts';
  const searchParams = useSearchParams();
  const { logEvent } = useAnalytics();
  const lastSearchSignatureRef = useRef<string>('');

  const [mobileFiltersOpen, setMobileFiltersOpen] = useState(false);

  const [filters, setFilters] = useState<ContractsUiFilters>(() =>
    getInitialFilters(new URLSearchParams(searchParams?.toString() ?? '')),
  );

  const debouncedQuery = useDebouncedValue(filters.query, 300);

  useEffect(() => {
    const params = new URLSearchParams();
    if (debouncedQuery) params.set('query', debouncedQuery);
    filters.categories.forEach((category) => params.append('category', category));
    filters.languages.forEach((language) => params.append('language', language));
    filters.tags.forEach((tag) => params.append('tag', tag));
    filters.networks.forEach((network) => params.append('network', network));
    if (filters.author) params.set('author', filters.author);
    if (filters.verified_only) params.set('verified_only', 'true');
    if (filters.sort_by) params.set('sort_by', filters.sort_by);
    if (filters.sort_order) params.set('sort_order', filters.sort_order);
    if (filters.page > 1) params.set('page', String(filters.page));
    params.set('page_size', String(filters.page_size));

    const next = params.toString();
    router.replace(next ? `${pathname}?${next}` : pathname, { scroll: false });
  }, [debouncedQuery, filters, pathname, router]);

  const apiParams = useMemo<ContractSearchParams>(
    () => ({
      query: debouncedQuery || undefined,
      categories: filters.categories.length > 0 ? filters.categories : undefined,
      languages: filters.languages.length > 0 ? filters.languages : undefined,
      tags: filters.tags.length > 0 ? filters.tags : undefined,
      author: filters.author || undefined,
      networks: filters.networks.length > 0 ? filters.networks : undefined,
      verified_only: filters.verified_only,
      sort_by: filters.sort_by,
      sort_order: filters.sort_order,
      page: filters.page,
      page_size: filters.page_size,
    }),
    [debouncedQuery, filters],
  );

  const { data, isLoading, isFetching } = useQuery<Awaited<ReturnType<typeof api.getContracts>>>({
    queryKey: ['contracts', apiParams],
    queryFn: () => api.getContracts(apiParams),
    placeholderData: (previousData) => previousData ?? EMPTY_CONTRACTS_RESPONSE,
  });

  const { data: stats } = useQuery({
    queryKey: ['stats'],
    queryFn: () => api.getStats(),
  });

  const paginationRange = useMemo(
    () => (data ? getPaginationRange(filters.page, data.total_pages) : []),
    [filters.page, data],
  );

  useEffect(() => {
    const payload = {
      keyword: debouncedQuery || '',
      categories: filters.categories,
      languages: filters.languages,
      networks: filters.networks,
      author: filters.author || undefined,
      verified_only: filters.verified_only,
      sort_by: filters.sort_by,
      page: filters.page,
      page_size: filters.page_size,
    };

    const hasSearchInput =
      Boolean(payload.keyword) ||
      payload.categories.length > 0 ||
      payload.languages.length > 0 ||
      payload.networks.length > 0 ||
      Boolean(payload.author) ||
      payload.verified_only ||
      payload.sort_by !== 'created_at' ||
      payload.page > 1;

    if (!hasSearchInput) return;

    const signature = JSON.stringify(payload);
    if (lastSearchSignatureRef.current === signature) return;
    lastSearchSignatureRef.current = signature;

    logEvent('search_performed', payload);
  }, [debouncedQuery, filters, logEvent]);

  const clearAllFilters = () =>
    setFilters((current) => ({
      ...current,
      query: '',
      categories: [],
      languages: [],
      tags: [],
      author: '',
      networks: [],
      verified_only: false,
      sort_by: 'created_at',
      sort_order: 'desc',
      page: 1,
    }));

  const activeFilterChips = useMemo(() => {
    const chips: Array<{ id: string; label: string; onRemove: () => void }> = [];

    if (debouncedQuery) {
      chips.push({
        id: 'query',
        label: `Search: ${debouncedQuery}`,
        onRemove: () => setFilters((current) => ({ ...current, query: '', page: 1 })),
      });
    }

    filters.categories.forEach((category) =>
      chips.push({
        id: `category:${category}`,
        label: `Category: ${category}`,
        onRemove: () =>
          setFilters((current) => ({
            ...current,
            categories: removeOne(current.categories, category),
            page: 1,
          })),
      }),
    );

    filters.languages.forEach((language) =>
      chips.push({
        id: `language:${language}`,
        label: `Language: ${language}`,
        onRemove: () =>
          setFilters((current) => ({
            ...current,
            languages: removeOne(current.languages, language),
            page: 1,
          })),
      }),
    );

    filters.tags.forEach((tag) =>
      chips.push({
        id: `tag:${tag}`,
        label: `Tag: ${tag}`,
        onRemove: () =>
          setFilters((current) => ({
            ...current,
            tags: removeOne(current.tags, tag),
            page: 1,
          })),
      }),
    );

    filters.networks.forEach((network) =>
      chips.push({
        id: `network:${network}`,
        label: `Network: ${network}`,
        onRemove: () =>
          setFilters((current) => ({
            ...current,
            networks: removeOne(current.networks, network),
            page: 1,
          })),
      }),
    );

    if (filters.author) {
      chips.push({
        id: 'author',
        label: `Author: ${filters.author}`,
        onRemove: () => setFilters((current) => ({ ...current, author: '', page: 1 })),
      });
    }

    if (filters.verified_only) {
      chips.push({
        id: 'verified',
        label: 'Verified only',
        onRemove: () =>
          setFilters((current) => ({ ...current, verified_only: false, page: 1 })),
      });
    }

    if (filters.sort_by !== 'created_at' || filters.sort_order !== 'desc') {
      chips.push({
        id: 'sort',
        label: `Sort: ${filters.sort_by.replace('_', ' ')} (${filters.sort_order})`,
        onRemove: () => setFilters((current) => ({ ...current, sort_by: 'created_at', sort_order: 'desc' })),
      });
    }

    return chips;
  }, [debouncedQuery, filters]);

  const filterPanel = (
    <FilterPanel
      categories={CATEGORY_OPTIONS}
      selectedCategories={filters.categories}
      onToggleCategory={(value) =>
        setFilters((current) => ({
          ...current,
          categories: toggleOne(current.categories, value),
          page: 1,
        }))
      }
      languages={LANGUAGE_OPTIONS}
      selectedLanguages={filters.languages}
      onToggleLanguage={(value) =>
        setFilters((current) => ({
          ...current,
          languages: toggleOne(current.languages, value),
          page: 1,
        }))
      }
      selectedNetworks={filters.networks}
      onToggleNetwork={(value) =>
        setFilters((current) => ({
          ...current,
          networks: toggleOne(current.networks, value),
          page: 1,
        }))
      }
      author={filters.author}
      onAuthorChange={(value) =>
        setFilters((current) => ({ ...current, author: value, page: 1 }))
      }
      verifiedOnly={filters.verified_only}
      onVerifiedChange={(value) =>
        setFilters((current) => ({ ...current, verified_only: value, page: 1 }))
      }
    />
  );

  return (
    <>
      {/* Hero header with grid pattern */}
      <section className="relative overflow-hidden border-b border-border">
        <div className="absolute inset-0 bg-grid-pattern opacity-5 text-primary" />
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-16 relative">
          <div className="text-center max-w-3xl mx-auto">
            <div className="inline-flex items-center gap-2 px-4 py-2 rounded-full bg-primary/10 text-primary text-sm font-medium mb-6">
              <Sparkles className="w-4 h-4" />
              Explore the Soroban Ecosystem
            </div>

            <h1 className="text-4xl sm:text-5xl font-bold mb-4 leading-tight">
              Browse <span className="text-gradient">Contracts</span>
            </h1>
            <p className="text-lg text-muted-foreground mb-10">
              Discover verified Soroban smart contracts on the Stellar network.
              Search, filter, and find the perfect building blocks for your project.
            </p>

            {/* Inline search */}
            <div className="max-w-2xl mx-auto mb-10">
              <div className="relative">
                <Search className="absolute left-4 top-1/2 -translate-y-1/2 w-5 h-5 text-muted-foreground" />
                <input
                  type="text"
                  value={filters.query}
                  onChange={(e) => setFilters((current) => ({ ...current, query: e.target.value, page: 1 }))}
                  placeholder="Search contracts by name, category, or tag..."
                  aria-label="Search contracts"
                  aria-keyshortcuts="/"
                  className="w-full pl-12 pr-24 py-4 rounded-xl border border-border bg-background text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary shadow-lg"
                />
                {filters.query && (
                  <button
                    type="button"
                    onClick={() => {
                      logEvent('search_performed', {
                        keyword: '',
                        action: 'clear_query',
                      });
                      setFilters((current) => ({ ...current, query: '', page: 1 }));
                    }}
                    className="absolute right-20 top-1/2 -translate-y-1/2 p-1.5 rounded-md text-muted-foreground hover:text-foreground transition-colors"
                    aria-label="Clear search"
                  >
                    <X className="w-4 h-4" />
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => setMobileFiltersOpen(true)}
                  className="md:hidden absolute right-2 top-1/2 -translate-y-1/2 px-4 py-2 rounded-lg bg-primary text-primary-foreground hover:opacity-90 transition-opacity font-medium text-sm"
                >
                  Filters
                </button>
                <div className="hidden md:flex absolute right-2 top-1/2 -translate-y-1/2 items-center gap-2">
                  <kbd className="px-2 py-1 rounded bg-muted text-muted-foreground text-xs font-mono border border-border">/</kbd>
                </div>
              </div>
            </div>

            {/* Stats row */}
            <div className="grid grid-cols-3 gap-4 max-w-lg mx-auto">
              <div className="bg-background rounded-xl p-4 border border-border shadow-sm">
                <div className="flex items-center justify-center gap-1.5 mb-1">
                  <Package className="w-4 h-4 text-primary" />
                  <span className="text-2xl font-bold">{stats?.total_contracts ?? '—'}</span>
                </div>
                <p className="text-xs text-muted-foreground">Contracts</p>
              </div>
              <div className="bg-background rounded-xl p-4 border border-border shadow-sm">
                <div className="flex items-center justify-center gap-1.5 mb-1">
                  <CheckCircle className="w-4 h-4 text-green-500" />
                  <span className="text-2xl font-bold">{stats?.verified_contracts ?? '—'}</span>
                </div>
                <p className="text-xs text-muted-foreground">Verified</p>
              </div>
              <div className="bg-background rounded-xl p-4 border border-border shadow-sm">
                <div className="flex items-center justify-center gap-1.5 mb-1">
                  <Users className="w-4 h-4 text-secondary" />
                  <span className="text-2xl font-bold">{stats?.total_publishers ?? '—'}</span>
                </div>
                <p className="text-xs text-muted-foreground">Publishers</p>
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* Main content */}
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
        {/* Toolbar */}
        <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 mb-6">
          <div className="flex items-center gap-3">
            <ResultsCount visibleCount={data?.items.length ?? 0} totalCount={data?.total ?? 0} />
            {isFetching && !isLoading && (
              <span className="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
                <div className="w-3 h-3 border-2 border-primary border-t-transparent rounded-full animate-spin" />
                Updating...
              </span>
            )}
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <SortDropdown
              value={filters.sort_by}
              onChange={(value) =>
                setFilters((current) => ({ ...current, sort_by: value, page: 1 }))
              }
              showRelevance={!!filters.query}
            />
            <select
              value={filters.sort_order}
              onChange={(e) => setFilters(prev => ({ ...prev, sort_order: e.target.value as 'asc' | 'desc', page: 1 }))}
              className="px-3 py-2 rounded-lg border border-border bg-background text-sm text-foreground focus:outline-none focus:ring-2 focus:ring-primary/20"
            >
              <option value="desc">Descending</option>
              <option value="asc">Ascending</option>
            </select>
            <button
              type="button"
              onClick={() => setMobileFiltersOpen(true)}
              className="md:hidden inline-flex items-center gap-2 px-3 py-2 rounded-lg border border-border text-sm text-foreground hover:bg-accent transition-colors"
            >
              <SlidersHorizontal className="w-4 h-4" />
              Filters
            </button>
          </div>
        </div>

        <ActiveFilters chips={activeFilterChips} onClearAll={clearAllFilters} />

        <div className="flex gap-8 mt-6">
          {/* Sidebar filters (desktop) */}
          <aside className="hidden md:block w-64 flex-shrink-0">
            <div className="gradient-border-card p-5 sticky top-20">
              <div className="flex items-center gap-2 mb-5">
                <Filter className="w-4 h-4 text-primary" />
                <h3 className="text-sm font-semibold text-foreground">Filters</h3>
              </div>
              {filterPanel}

              <div className="mt-5 pt-4 border-t border-border">
                <div className="w-full">
                  <TagAutocomplete
                    onSelect={(tag) =>
                      setFilters((current) => {
                        if (current.tags.includes(tag.name)) return current;
                        return {
                          ...current,
                          tags: [...current.tags, tag.name],
                          page: 1,
                        };
                      })
                    }
                    placeholder="Filter by tag..."
                  />
                </div>
              </div>
            </div>
          </aside>

          {/* Results grid */}
          <div className="flex-1 min-w-0">
            {isLoading ? (
              <div className="grid grid-cols-1 lg:grid-cols-2 xl:grid-cols-3 gap-6 mb-8">
                {Array.from({ length: 6 }).map((_, i) => (
                  <ContractCardSkeleton key={i} />
                ))}
              </div>
            ) : data && data.items.length > 0 ? (
              <>
                <div className="grid grid-cols-1 lg:grid-cols-2 xl:grid-cols-3 gap-6 mb-8">
                  {data.items.map((contract: Contract) => (
                    <ContractCard key={contract.id} contract={contract} />
                  ))}
                </div>

                {data.total_pages > 1 && (
                  <div className="flex flex-wrap items-center justify-center gap-2 py-4">
                    <button
                      onClick={() =>
                        setFilters((current) => ({ ...current, page: Math.max(1, current.page - 1) }))
                      }
                      disabled={filters.page <= 1}
                      className="px-4 py-2 rounded-lg border border-border text-foreground disabled:opacity-50 disabled:cursor-not-allowed hover:bg-accent transition-colors text-sm font-medium"
                    >
                      Previous
                    </button>

                    {paginationRange.map((item, index) => {
                      if (item === 'ellipsis') {
                        return (
                          <span
                            key={`ellipsis-${index}`}
                            className="px-2 text-sm text-muted-foreground"
                            aria-hidden="true"
                          >
                            ...
                          </span>
                        );
                      }

                      const isActive = item === filters.page;

                      return (
                        <button
                          key={item}
                          type="button"
                          onClick={() =>
                            setFilters((current) => ({
                              ...current,
                              page: Math.min(data.total_pages, Math.max(1, item)),
                            }))
                          }
                          aria-current={isActive ? 'page' : undefined}
                          className={
                            isActive
                              ? 'px-3 py-2 rounded-lg bg-primary text-primary-foreground font-medium text-sm btn-glow'
                              : 'px-3 py-2 rounded-lg border border-border text-foreground hover:bg-accent transition-colors text-sm'
                          }
                        >
                          {item}
                        </button>
                      );
                    })}

                    <button
                      onClick={() =>
                        setFilters((current) => ({ ...current, page: current.page + 1 }))
                      }
                      disabled={filters.page >= data.total_pages}
                      className="px-4 py-2 rounded-lg border border-border text-foreground disabled:opacity-50 disabled:cursor-not-allowed hover:bg-accent transition-colors text-sm font-medium"
                    >
                      Next
                    </button>
                  </div>
                )}
              </>
            ) : (
              <div className="text-center py-20 gradient-border-card">
                <div className="inline-flex items-center justify-center w-16 h-16 rounded-full bg-primary/10 mb-6">
                  <Package className="w-8 h-8 text-primary" />
                </div>
                <h3 className="text-xl font-semibold mb-2">No contracts found</h3>
                <p className="text-muted-foreground mb-6 max-w-md mx-auto">
                  No contracts match the current filters. Try adjusting your search or clearing filters.
                </p>
                <button
                  type="button"
                  onClick={() => {
                    logEvent('search_performed', {
                      keyword: '',
                      action: 'clear_all_filters',
                    });
                    clearAllFilters();
                  }}
                  className="btn-glow px-6 py-2.5 rounded-lg bg-primary text-primary-foreground font-medium"
                >
                  Clear all filters
                </button>
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Mobile Filters Drawer */}
      {mobileFiltersOpen && (
        <div className="md:hidden fixed inset-0 z-50 bg-black/60 backdrop-blur-sm">
          <div className="absolute right-0 top-0 h-full w-[88%] max-w-sm bg-background border-l border-border p-5 shadow-2xl animate-in slide-in-from-right duration-300 overflow-y-auto">
            <div className="flex items-center justify-between mb-6">
              <div className="flex items-center gap-2">
                <Filter className="w-4 h-4 text-primary" />
                <h2 className="text-lg font-semibold">Filters</h2>
              </div>
              <button
                type="button"
                onClick={() => setMobileFiltersOpen(false)}
                className="p-1.5 rounded-lg text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
                aria-label="Close filters"
              >
                <X className="w-5 h-5" />
              </button>
            </div>
            {filterPanel}

            <div className="mt-5 pt-4 border-t border-border">
              <TagAutocomplete
                onSelect={(tag) =>
                  setFilters((current) => {
                    if (current.tags.includes(tag.name)) return current;
                    return {
                      ...current,
                      tags: [...current.tags, tag.name],
                      page: 1,
                    };
                  })
                }
                placeholder="Filter by tag..."
              />
            </div>

            <button
              type="button"
              onClick={() => setMobileFiltersOpen(false)}
              className="mt-8 w-full px-4 py-3 rounded-xl bg-primary text-primary-foreground hover:opacity-90 transition-opacity font-medium btn-glow"
            >
              Show results
            </button>
          </div>
        </div>
      )}
    </>
  );
}
