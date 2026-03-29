"use client";

import { Suspense, useState, useEffect, useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import type {
  Network,
  DependencyTreeNode,
  GraphNode,
  GraphEdge,
  ContractVersion,
  ContractChangelogEntry,
} from "@/lib/api";
import ExampleGallery from "@/components/ExampleGallery";
import DependencyGraph from "@/components/DependencyGraph";
import {
  ArrowLeft,
  CheckCircle2,
  Globe,
  Tag,
  Search,
  BarChart3,
  History,
  Database,
  Code2,
  Layers,
  MessageSquare,
} from "lucide-react";
import Link from "next/link";
import { useCopy } from "@/hooks/useCopy";
import CodeCopyButton from "@/components/CodeCopyButton";
import { useParams, useSearchParams } from "next/navigation";
import { useAnalytics } from "@/hooks/useAnalytics";
import FormalVerificationPanel from "@/components/FormalVerificationPanel";
import InteractionHistorySection from "@/components/InteractionHistorySection";
import Navbar from "@/components/Navbar";
import MaintenanceBanner from "@/components/MaintenanceBanner";
import CustomMetricsPanel from "@/components/CustomMetricsPanel";
import DeprecationBanner from "@/components/DeprecationBanner";
import ReleaseNotesPanel from "@/components/ReleaseNotesPanel";
import ContractComments from "@/components/ContractComments";
import { useContractAutoRefresh } from "@/hooks/useContractAutoRefresh";

const NETWORKS: Network[] = ["mainnet", "testnet", "futurenet"];
const TAB_IDS = ["overview", "abi", "source", "deployments", "analytics", "history", "discussion"] as const;
type TabId = (typeof TAB_IDS)[number];

// TODO: Replace with real API call when maintenance endpoint is available
const maintenanceStatus: { is_maintenance: boolean; current_window: null } = {
  is_maintenance: false,
  current_window: null,
};

/** Flatten a recursive DependencyTreeNode[] into GraphNode[] + GraphEdge[]. */
function flattenDependencyTree(
  tree: DependencyTreeNode | DependencyTreeNode[],
  network: Network = "mainnet"
): { nodes: GraphNode[]; edges: GraphEdge[] } {
  const nodes: GraphNode[] = [];
  const edges: GraphEdge[] = [];
  const seen = new Set<string>();

  function walk(node: DependencyTreeNode, parentId?: string) {
    if (!seen.has(node.contract_id)) {
      seen.add(node.contract_id);
      nodes.push({
        id: node.contract_id,
        contract_id: node.contract_id,
        name: node.name,
        network,
        is_verified: false,
        tags: [],
      });
    }
    if (parentId) {
      edges.push({
        source: parentId,
        target: node.contract_id,
        dependency_type: node.constraint_to_parent || "dependency",
      });
    }
    for (const child of node.dependencies) {
      walk(child, node.contract_id);
    }
  }

  const roots = Array.isArray(tree) ? tree : [tree];
  for (const root of roots) {
    walk(root);
  }
  return { nodes, edges };
}

function normalizeRawSourceUrl(input: string): string {
  try {
    const url = new URL(input);
    if (url.hostname === "github.com") {
      const parts = url.pathname.split("/").filter(Boolean);
      const blobIdx = parts.indexOf("blob");
      if (parts.length >= 5 && blobIdx === 2) {
        const owner = parts[0];
        const repo = parts[1];
        const branch = parts[3];
        const path = parts.slice(4).join("/");
        return `https://raw.githubusercontent.com/${owner}/${repo}/${branch}/${path}`;
      }
    }
  } catch {
    return input;
  }
  return input;
}

const RUST_KEYWORDS = new Set([
  "fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "use", "mod", "match",
  "if", "else", "for", "while", "loop", "return", "async", "await", "where", "crate",
  "Self", "self", "const", "static", "type", "move", "ref", "in", "as",
]);

function HighlightedRustCode({ code, query }: { code: string; query: string }) {
  const lowered = query.trim().toLowerCase();
  const filteredLines = useMemo(() => {
    const lines = code.split("\n");
    if (!lowered) return lines;
    return lines.filter((line) => line.toLowerCase().includes(lowered));
  }, [code, lowered]);

  return (
    <pre className="overflow-x-auto rounded-xl border border-border bg-card p-4 text-xs leading-6 font-mono text-foreground">
      {filteredLines.map((line, idx) => {
        const parts = line.split(/(\s+)/);
        let inComment = false;
        return (
          <div key={`${idx}-${line.slice(0, 16)}`}>
            {parts.map((token, tokenIdx) => {
              if (token.startsWith("//")) inComment = true;
              if (/^\s+$/.test(token)) {
                return <span key={tokenIdx}>{token}</span>;
              }

              const tokenWord = token.replace(/[^A-Za-z0-9_]/g, "");
              const isKeyword = RUST_KEYWORDS.has(tokenWord);
              const matchesQuery = lowered && token.toLowerCase().includes(lowered);

              let className = "";
              if (inComment) className = "text-emerald-400";
              else if (token.startsWith("\"") || token.endsWith("\"")) className = "text-amber-300";
              else if (isKeyword) className = "text-sky-300 font-semibold";

              if (matchesQuery) {
                return (
                  <mark key={tokenIdx} className={`rounded px-0.5 bg-primary/30 text-foreground ${className}`}>
                    {token}
                  </mark>
                );
              }

              return (
                <span key={tokenIdx} className={className}>
                  {token}
                </span>
              );
            })}
          </div>
        );
      })}
    </pre>
  );
}

function ContractDetailsContent() {
  const params = useParams<{ id?: string | string[] }>() ?? {};
  const searchParams = useSearchParams();
  const idParam = params.id;
  const id = Array.isArray(idParam) ? idParam[0] : idParam;
  const { copy: copyHeader, copied: copiedHeader } = useCopy();
  const { copy: copySidebar, copied: copiedSidebar } = useCopy();
  const networkFromUrl = searchParams?.get("network") as Network | null;
  const [selectedNetwork, setSelectedNetwork] = useState<Network>(
    networkFromUrl && NETWORKS.includes(networkFromUrl) ? networkFromUrl : "mainnet"
  );
  const [activeTab, setActiveTab] = useState<TabId>("overview");
  const [tabSearch, setTabSearch] = useState("");

  const tabMeta: Record<TabId, { label: string; icon: React.ComponentType<{ className?: string }> }> = {
    overview: { label: "Overview", icon: Layers },
    abi: { label: "ABI", icon: Database },
    source: { label: "Source Code", icon: Code2 },
    deployments: { label: "Deployments", icon: Globe },
    analytics: { label: "Analytics", icon: BarChart3 },
    history: { label: "History", icon: History },
    discussion: { label: "Discussion", icon: MessageSquare },
  };

  // Subscribe to real-time contract updates
  useContractAutoRefresh(id);

  const {
    data: contract,
    isLoading,
    error,
  } = useQuery({
    queryKey: ["contract", id],
    queryFn: () => api.getContract(id!),
    enabled: !!id,
  });

  const { data: dependencies, isLoading: depsLoading } = useQuery({
    queryKey: ["contract-dependencies", id],
    queryFn: () => api.getContractDependencies(id!),
    enabled: !!id && !!contract && activeTab === "overview",
  });

  const { data: versions = [] } = useQuery({
    queryKey: ["contract-versions", id],
    queryFn: () => api.getContractVersions(id!),
    enabled: !!id && !!contract && ["source", "deployments", "history", "abi"].includes(activeTab),
  });

  const latestVersion = useMemo(() => {
    if (!versions.length) return null;
    return [...versions].sort((a: ContractVersion, b: ContractVersion) =>
      new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
    )[0];
  }, [versions]);

  const { data: abiResponse, isLoading: abiLoading } = useQuery({
    queryKey: ["contract-abi", id, latestVersion?.version],
    queryFn: () => api.getContractAbi(id!, latestVersion?.version),
    enabled: !!id && !!contract && activeTab === "abi",
  });

  const { data: analyticsData } = useQuery({
    queryKey: ["contract-analytics-summary", id],
    queryFn: () => api.getContractAnalytics(id!),
    enabled: !!id && !!contract && ["analytics", "deployments"].includes(activeTab),
  });

  const { data: changelog } = useQuery({
    queryKey: ["contract-changelog", id],
    queryFn: () => api.getContractChangelog(id!),
    enabled: !!id && !!contract && activeTab === "history",
  });

  const sourceUrl = latestVersion?.source_url;
  const sourceQueryUrl = sourceUrl ? normalizeRawSourceUrl(sourceUrl) : undefined;
  const {
    data: sourceCode,
    isLoading: sourceLoading,
    error: sourceError,
  } = useQuery({
    queryKey: ["contract-source", id, sourceQueryUrl],
    queryFn: async () => {
      if (!sourceQueryUrl) return "";
      const res = await fetch(sourceQueryUrl);
      if (!res.ok) throw new Error("Unable to fetch source from source_url");
      return res.text();
    },
    enabled: !!id && !!contract && activeTab === "source" && !!sourceQueryUrl,
  });

  const depGraph = useMemo(
    () => (dependencies ? flattenDependencyTree(dependencies as DependencyTreeNode | DependencyTreeNode[], selectedNetwork) : null),
    [dependencies, selectedNetwork]
  );

  const loweredSearch = tabSearch.trim().toLowerCase();
  const filteredVersions = useMemo(() => {
    if (!loweredSearch) return versions;
    return versions.filter((version) =>
      [version.version, version.wasm_hash, version.commit_hash, version.release_notes, version.source_url]
        .filter(Boolean)
        .some((value) => String(value).toLowerCase().includes(loweredSearch))
    );
  }, [versions, loweredSearch]);

  const filteredHistory = useMemo(() => {
    const entries = changelog?.entries ?? [];
    if (!loweredSearch) return entries;
    return entries.filter((entry: ContractChangelogEntry) =>
      [entry.version, entry.commit_hash, entry.release_notes, entry.source_url, ...entry.breaking_changes]
        .filter(Boolean)
        .some((value) => String(value).toLowerCase().includes(loweredSearch))
    );
  }, [changelog, loweredSearch]);

  const { logEvent } = useAnalytics();

  useEffect(() => {
    if (!error) return;
    logEvent("error_event", {
      source: "contract_details",
      contract_id: id,
      message: "Failed to load contract details",
    });
  }, [error, id, logEvent]);

  const { data: deprecationInfo } = useQuery({
    queryKey: ["contract-deprecation", id],
    queryFn: () => api.getDeprecationInfo(id!),
    enabled: !!id && !!contract,
  });

  if (!id) {
    return (
      <div className="min-h-screen bg-background text-foreground">
        <Navbar />
        <div className="max-w-4xl mx-auto px-4 py-10">
          <div className="rounded-2xl border border-border bg-card p-6">
            <div className="text-sm font-semibold text-foreground">Missing contract id</div>
            <div className="mt-1 text-sm text-muted-foreground">Open this page from the contracts list.</div>
            <div className="mt-4">
              <Link
                href="/contracts"
                className="inline-flex items-center gap-2 rounded-xl border border-border bg-background px-3 py-2 text-sm font-semibold text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
              >
                Browse contracts
              </Link>
            </div>
          </div>
        </div>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-12">
        <div className="animate-pulse space-y-8">
          <div className="h-8 bg-muted rounded w-1/3" />
          <div className="h-4 bg-muted rounded w-1/2" />
          <div className="h-64 bg-muted rounded-xl" />
        </div>
      </div>
    );
  }

  if (error || !contract) {
    return (
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-12">
        <div className="p-4 bg-red-500/10 border border-red-500/20 text-red-500 rounded-xl">
          Failed to load contract details
        </div>
      </div>
    );
  }

  const configForNetwork = contract.network_configs?.[selectedNetwork];
  const displayContractId = configForNetwork?.contract_id ?? contract.contract_id;
  const displayVerified = configForNetwork?.is_verified ?? contract.is_verified;

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8 animate-in fade-in duration-500">
      <Link
        href="/contracts"
        className="inline-flex items-center gap-2 text-muted-foreground hover:text-foreground mb-8 transition-colors"
      >
        <ArrowLeft className="w-4 h-4" />
        Back to contracts
      </Link>

      {/* Maintenance Banner */}
      {maintenanceStatus?.is_maintenance && maintenanceStatus.current_window && (
        <MaintenanceBanner window={maintenanceStatus.current_window} />
      )}

      {/* Deprecation Banner */}
      {deprecationInfo && <DeprecationBanner info={deprecationInfo} />}

      {/* Header */}
      <div className="mb-12">
        <div className="flex items-start justify-between mb-4">
          <div>
            <h1 className="text-4xl font-bold text-foreground mb-2">
              {contract.name}
            </h1>
            <div className="flex items-center gap-3 text-muted-foreground">
              <span className="flex items-center gap-2 font-mono bg-accent px-2 py-1 rounded-lg text-sm">
                <span>{displayContractId}</span>
                <CodeCopyButton copied={copiedHeader} onCopy={() => copyHeader(displayContractId)} />
              </span>
              {displayVerified && (
                <span className="flex items-center gap-1 text-green-600 dark:text-green-400 text-sm font-medium">
                  <CheckCircle2 className="w-4 h-4" />
                  Verified
                </span>
              )}
            </div>
          </div>

          {/* Network tabs (Issue #43) */}
          <div className="flex gap-1 p-1 bg-accent rounded-xl w-fit">
            {NETWORKS.map((net) => {
              const hasConfig = !!contract.network_configs?.[net];
              return (
                <button
                  key={net}
                  type="button"
                  onClick={() => setSelectedNetwork(net)}
                  className={`px-4 py-2 rounded-lg text-sm font-medium capitalize transition-all ${selectedNetwork === net
                      ? "bg-card text-foreground shadow-sm"
                      : "text-muted-foreground hover:text-foreground"
                    } ${!hasConfig ? "opacity-60" : ""}`}
                >
                  {net}
                </button>
              );
            })}
          </div>

          <div className="flex gap-2">
            {/* Publisher actions/links could go here */}
          </div>
        </div>

        {contract.description && (
          <p className="text-xl text-muted-foreground max-w-3xl mb-6">
            {contract.description}
          </p>
        )}

        <div className="flex flex-wrap gap-2">
          {contract.tags.map((tag) => (
            <span
              key={tag}
              className="inline-flex items-center gap-1 px-3 py-1 rounded-full bg-primary/10 text-primary text-sm font-medium"
            >
              <Tag className="w-3 h-3" />
              {tag}
            </span>
          ))}
        </div>
      </div>

      <div className="space-y-5">
        <div className="md:hidden">
          <label className="sr-only" htmlFor="contract-tab-select">Contract tab</label>
          <select
            id="contract-tab-select"
            className="w-full rounded-xl border border-border bg-card p-3 text-sm text-foreground"
            value={activeTab}
            onChange={(e) => {
              setActiveTab(e.target.value as TabId);
              setTabSearch("");
            }}
          >
            {TAB_IDS.map((tabId) => (
              <option key={tabId} value={tabId}>{tabMeta[tabId].label}</option>
            ))}
          </select>
        </div>

        <div className="hidden md:flex flex-wrap gap-2 p-2 rounded-2xl border border-border bg-card">
          {TAB_IDS.map((tabId) => {
            const Icon = tabMeta[tabId].icon;
            return (
              <button
                key={tabId}
                type="button"
                onClick={() => {
                  setActiveTab(tabId);
                  setTabSearch("");
                }}
                className={`inline-flex items-center gap-2 rounded-xl px-4 py-2 text-sm font-medium transition-colors ${activeTab === tabId
                    ? "bg-primary text-primary-foreground"
                    : "text-muted-foreground hover:text-foreground hover:bg-accent"
                  }`}
              >
                <Icon className="w-4 h-4" />
                {tabMeta[tabId].label}
              </button>
            );
          })}
        </div>

        <div className="relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
          <input
            type="search"
            value={tabSearch}
            onChange={(e) => setTabSearch(e.target.value)}
            placeholder={`Search in ${tabMeta[activeTab].label}`}
            className="w-full rounded-xl border border-border bg-card pl-10 pr-3 py-2.5 text-sm text-foreground"
          />
        </div>

        {activeTab === "overview" && (
          <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
            <div className="lg:col-span-2 space-y-6">
              {depsLoading ? (
                <section className="bg-card rounded-2xl p-8 border border-border">
                  <div className="animate-pulse space-y-4">
                    <div className="h-8 bg-muted rounded w-1/3" />
                    <div className="h-96 bg-muted rounded-lg" />
                  </div>
                </section>
              ) : depGraph && depGraph.nodes.length > 0 ? (
                <section className="bg-card rounded-2xl border border-border p-4 md:p-6">
                  <h2 className="text-xl font-semibold text-foreground mb-4">Dependency Graph</h2>
                  <div className="h-[520px]">
                    <DependencyGraph nodes={depGraph.nodes} edges={depGraph.edges} searchQuery={tabSearch} />
                  </div>
                </section>
              ) : (
                <section className="bg-card rounded-2xl border border-border p-6">
                  <h2 className="text-xl font-semibold text-foreground mb-2">Dependency Graph</h2>
                  <p className="text-sm text-muted-foreground">No dependency graph available for this contract.</p>
                </section>
              )}

              <section>
                <ExampleGallery contractId={contract.id} />
              </section>
            </div>

            <div className="space-y-6">
              <div className="bg-card rounded-2xl border border-border p-6">
                <h3 className="font-semibold text-foreground mb-4">Key Info</h3>
                <dl className="space-y-3 text-sm">
                  <div>
                    <dt className="text-muted-foreground">Network</dt>
                    <dd className="font-medium text-foreground capitalize">{selectedNetwork}</dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">Address</dt>
                    <dd className="flex items-center justify-between gap-2 font-mono text-xs text-foreground break-all">
                      <span>{displayContractId}</span>
                      <CodeCopyButton copied={copiedSidebar} onCopy={() => copySidebar(displayContractId)} />
                    </dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">Published</dt>
                    <dd className="font-medium text-foreground">{new Date(contract.created_at).toLocaleDateString()}</dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">Last Updated</dt>
                    <dd className="font-medium text-foreground">{new Date(contract.updated_at).toLocaleDateString()}</dd>
                  </div>
                </dl>
              </div>

              <Link
                href={`/contracts/${contract.id}/api-docs`}
                className="flex items-center gap-3 w-full px-4 py-3 rounded-xl border border-border bg-card hover:bg-primary/5 hover:border-primary/30 text-muted-foreground hover:text-primary transition-all group"
              >
                <Globe className="w-5 h-5 text-muted-foreground group-hover:text-primary transition-colors" />
                <div>
                  <div className="text-sm font-medium">API Docs</div>
                  <div className="text-xs text-muted-foreground">OpenAPI / Swagger UI</div>
                </div>
              </Link>

              <FormalVerificationPanel contractId={contract.id} />
            </div>
          </div>
        )}

        {activeTab === "abi" && (
          <section className="bg-card rounded-2xl border border-border p-6 space-y-4">
            <h2 className="text-xl font-semibold text-foreground">ABI</h2>
            {abiLoading ? (
              <div className="h-64 animate-pulse bg-muted rounded" />
            ) : (
              <pre className="overflow-x-auto rounded-xl border border-border bg-background p-4 text-xs leading-6">
                {JSON.stringify(abiResponse?.abi ?? {}, null, 2)
                  .split("\n")
                  .filter((line) => !loweredSearch || line.toLowerCase().includes(loweredSearch))
                  .map((line, idx) => (
                    <div key={`${idx}-${line.slice(0, 12)}`}>
                      {line || " "}
                    </div>
                  ))}
              </pre>
            )}
          </section>
        )}

        {activeTab === "source" && (
          <section className="bg-card rounded-2xl border border-border p-6 space-y-4">
            <div className="flex items-center justify-between gap-4">
              <h2 className="text-xl font-semibold text-foreground">Source Code</h2>
              {sourceUrl && (
                <a
                  href={sourceUrl}
                  target="_blank"
                  rel="noreferrer"
                  className="text-sm text-primary hover:underline"
                >
                  Open Source URL
                </a>
              )}
            </div>

            {!sourceUrl && (
              <p className="text-sm text-muted-foreground">No source URL available for the latest version.</p>
            )}

            {sourceLoading && <div className="h-64 animate-pulse bg-muted rounded" />}
            {sourceError && (
              <p className="text-sm text-amber-500">
                Unable to fetch source code from remote URL. You can still open the source URL directly.
              </p>
            )}
            {sourceCode && <HighlightedRustCode code={sourceCode} query={tabSearch} />}
          </section>
        )}

        {activeTab === "deployments" && (
          <section className="bg-card rounded-2xl border border-border p-6 space-y-6">
            <h2 className="text-xl font-semibold text-foreground">Deployments</h2>

            <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
              <div className="rounded-xl border border-border p-4 bg-background">
                <p className="text-xs text-muted-foreground uppercase tracking-wide">Total deployments</p>
                <p className="text-2xl font-bold text-foreground">{analyticsData?.deployments.count ?? 0}</p>
              </div>
              <div className="rounded-xl border border-border p-4 bg-background">
                <p className="text-xs text-muted-foreground uppercase tracking-wide">Unique users</p>
                <p className="text-2xl font-bold text-foreground">{analyticsData?.deployments.unique_users ?? 0}</p>
              </div>
              <div className="rounded-xl border border-border p-4 bg-background">
                <p className="text-xs text-muted-foreground uppercase tracking-wide">Versions</p>
                <p className="text-2xl font-bold text-foreground">{versions.length}</p>
              </div>
            </div>

            <div className="space-y-3">
              <h3 className="text-sm font-semibold text-foreground uppercase tracking-wide">Deployment Timeline</h3>
              <div className="space-y-3">
                {filteredVersions.map((version) => (
                  <div key={version.id} className="rounded-xl border border-border p-4 bg-background">
                    <div className="flex items-center justify-between gap-2">
                      <p className="font-semibold text-foreground">v{version.version}</p>
                      <time className="text-xs text-muted-foreground">{new Date(version.created_at).toLocaleString()}</time>
                    </div>
                    <p className="text-xs text-muted-foreground mt-1 font-mono break-all">{version.wasm_hash}</p>
                    {version.release_notes && (
                      <p className="text-sm text-muted-foreground mt-2">{version.release_notes}</p>
                    )}
                  </div>
                ))}
                {filteredVersions.length === 0 && (
                  <p className="text-sm text-muted-foreground">No deployment timeline entries match this search.</p>
                )}
              </div>
            </div>
          </section>
        )}

        {activeTab === "analytics" && (
          <div className="space-y-6">
            <section className="bg-card rounded-2xl border border-border p-6">
              <h2 className="text-xl font-semibold text-foreground mb-4">Usage Analytics</h2>
              <p className="text-sm text-muted-foreground">
                Use the filters inside the analytics tables and charts below to search account and method activity.
              </p>
            </section>
            <InteractionHistorySection contractId={contract.id} />
            <CustomMetricsPanel contractId={contract.id} />
          </div>
        )}

        {activeTab === "history" && (
          <div className="space-y-6">
            <section className="bg-card rounded-2xl border border-border p-6 space-y-4">
              <h2 className="text-xl font-semibold text-foreground">Update History</h2>
              <div className="space-y-3">
                {filteredHistory.map((entry: ContractChangelogEntry) => (
                  <article key={`${entry.version}-${entry.created_at}`} className="rounded-xl border border-border p-4 bg-background">
                    <div className="flex flex-wrap items-center justify-between gap-2 mb-1.5">
                      <p className="font-semibold text-foreground">v{entry.version}</p>
                      <time className="text-xs text-muted-foreground">{new Date(entry.created_at).toLocaleString()}</time>
                    </div>
                    {entry.release_notes && <p className="text-sm text-muted-foreground mb-2">{entry.release_notes}</p>}
                    {entry.breaking && (
                      <div className="rounded-lg bg-red-500/10 text-red-500 px-3 py-2 text-xs">
                        Breaking changes detected
                      </div>
                    )}
                    {entry.breaking_changes.length > 0 && (
                      <ul className="mt-2 list-disc ml-4 text-xs text-muted-foreground space-y-1">
                        {entry.breaking_changes.map((change) => (
                          <li key={change}>{change}</li>
                        ))}
                      </ul>
                    )}
                  </article>
                ))}
                {filteredHistory.length === 0 && (
                  <p className="text-sm text-muted-foreground">No history entries match this search.</p>
                )}
              </div>
            </section>
            <ReleaseNotesPanel contractId={contract.id} />
          </div>
        )}

        {activeTab === "discussion" && (
          <section className="bg-card rounded-2xl border border-border p-6">
            <ContractComments contractId={contract.id} />
          </section>
        )}
      </div>
    </div>
  );
}

export default function ContractPage() {
  return (
    <div className="min-h-screen bg-background text-foreground">
      <Navbar />
      <Suspense fallback={null}>
        <ContractDetailsContent />
      </Suspense>
    </div>
  );
}
