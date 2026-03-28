"use client";

import { Suspense, useState, useEffect, useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import type { Network, DependencyTreeNode, GraphNode, GraphEdge } from "@/lib/api";
import ExampleGallery from "@/components/ExampleGallery";
import DependencyGraph from "@/components/DependencyGraph";
import {
  ArrowLeft,
  CheckCircle2,
  Globe,
  Tag,
  GitCompare,
  FlaskConical,
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
import { useContractAutoRefresh } from "@/hooks/useContractAutoRefresh";

const NETWORKS: Network[] = ["mainnet", "testnet", "futurenet"];

// TODO: Replace with real API call when maintenance endpoint is available
const maintenanceStatus: { is_maintenance: boolean; current_window: null } = {
  is_maintenance: false,
  current_window: null,
};

/** Flatten a recursive DependencyTreeNode[] into GraphNode[] + GraphEdge[]. */
function flattenDependencyTree(
  tree: DependencyTreeNode[],
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

  for (const root of tree) {
    walk(root);
  }
  return { nodes, edges };
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
  // Subscribe to real-time contract updates
  useContractAutoRefresh(id);

  const {
    data: contract,
    isLoading,
    error,
  } = useQuery({
    queryKey: ["contract", id],
    queryFn: () => api.getContract(id),
  });

  const { data: dependencies, isLoading: depsLoading } = useQuery({
    queryKey: ["contract-dependencies", id],
    queryFn: () => api.getContractDependencies(id),
    enabled: !!contract,
  });

  const depGraph = useMemo(
    () => (dependencies ? flattenDependencyTree(dependencies, selectedNetwork) : null),
    [dependencies, selectedNetwork]
  );

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
    queryFn: () => api.getDeprecationInfo(id),
    enabled: !!contract,
  });

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

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-8">
        {/* Main Content */}
        <div className="lg:col-span-2 space-y-12">
          {/* Dependency Graph */}
          {depsLoading ? (
            <section className="bg-card rounded-2xl p-8">
              <div className="animate-pulse space-y-4">
                <div className="h-8 bg-muted rounded w-1/3" />
                <div className="h-96 bg-muted rounded-lg" />
              </div>
            </section>
          ) : depGraph && depGraph.nodes.length > 0 ? (
            <section>
              <DependencyGraph
                nodes={depGraph.nodes}
                edges={depGraph.edges}
              />
            </section>
          ) : null}

          {/* Examples Gallery */}
          <section>
            <ExampleGallery contractId={contract.id} />
          </section>

          {/* Interaction History (Issue #46) */}
          <InteractionHistorySection contractId={contract.id} />
          {/* Custom Metrics */}
          <CustomMetricsPanel contractId={contract.id} />
        </div>

        {/* Sidebar */}
        <div className="space-y-6">
          <div className="bg-card rounded-2xl border border-border p-6">
            <h3 className="font-semibold text-foreground mb-4">
              Contract Details
            </h3>

            <dl className="space-y-3 text-sm">
              <div>
                <dt className="text-muted-foreground">Network</dt>
                <dd className="font-medium text-foreground capitalize">
                  {selectedNetwork}
                </dd>
              </div>
              {configForNetwork && (
                <>
                  <div>
                    <dt className="text-muted-foreground">Contract address</dt>
                    <dd className="flex items-center justify-between gap-2 font-mono text-xs text-foreground break-all">
                      <span>{displayContractId}</span>
                      <CodeCopyButton copied={copiedSidebar} onCopy={() => copySidebar(displayContractId)} />
                    </dd>
                  </div>
                  {(configForNetwork.min_version ?? configForNetwork.max_version) && (
                    <div>
                      <dt className="text-muted-foreground">Version range</dt>
                      <dd className="font-medium text-foreground">
                        {[configForNetwork.min_version, configForNetwork.max_version]
                          .filter(Boolean)
                          .join(" – ") || "—"}
                      </dd>
                    </div>
                  )}
                </>
              )}
              <div>
                <dt className="text-muted-foreground">Published</dt>
                <dd className="font-medium text-foreground">
                  {new Date(contract.created_at).toLocaleDateString()}
                </dd>
              </div>
              <div>
                <dt className="text-muted-foreground">
                  Last Updated
                </dt>
                <dd className="font-medium text-foreground">
                  {new Date(contract.updated_at).toLocaleDateString()}
                </dd>
              </div>
            </dl>
          </div>

          {/* API Documentation (OpenAPI / Swagger) */}
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

          {/* Compatibility Matrix link */}
          <Link
            href={`/contracts/${contract.id}/compatibility`}
            className="flex items-center gap-3 w-full px-4 py-3 rounded-xl border border-border bg-card hover:bg-primary/5 hover:border-primary/30 text-muted-foreground hover:text-primary transition-all group"
          >
            <GitCompare className="w-5 h-5 text-muted-foreground group-hover:text-primary transition-colors" />
            <div>
              <div className="text-sm font-medium">Compatibility Matrix</div>
              <div className="text-xs text-muted-foreground">View version compatibility</div>
            </div>
          </Link>

          {/* SDK Compatibility Testing link (Issue #261) */}
          <Link
            href={`/contracts/${contract.id}/compatibility-testing`}
            className="flex items-center gap-3 w-full px-4 py-3 rounded-xl border border-border bg-card hover:bg-secondary/5 hover:border-secondary/30 text-muted-foreground hover:text-secondary transition-all group"
          >
            <FlaskConical className="w-5 h-5 text-muted-foreground group-hover:text-secondary transition-colors" />
            <div>
              <div className="text-sm font-medium">SDK Compatibility Testing</div>
              <div className="text-xs text-muted-foreground">Test across SDK & runtime versions</div>
            </div>
          </Link>

          {/* Formal Verification Panel */}
          <FormalVerificationPanel contractId={contract.id} />

          {/* Release Notes Panel */}
          <ReleaseNotesPanel contractId={contract.id} />
        </div>
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
