'use client';

import { useQuery } from '@tanstack/react-query';
import { api, GraphNode, GraphEdge } from '@/lib/api';
import DependencyGraph from '@/components/DependencyGraph';
import GraphControls from '@/components/GraphControls';
import { useState, useCallback, useRef, useMemo, useEffect } from 'react';
import { AlertCircle, Sparkles, ExternalLink, X } from 'lucide-react';
import { useAnalytics } from '@/hooks/useAnalytics';
import type { DependencyGraphHandle } from '@/components/DependencyGraph';
import { useRouter } from 'next/navigation';

// Generate synthetic demo data for testing at scale
function generateDemoData(nodeCount: number): { nodes: GraphNode[]; edges: GraphEdge[] } {
    const networks: ('mainnet' | 'testnet' | 'futurenet')[] = ['mainnet', 'testnet', 'futurenet'];
    const categories = ['DeFi', 'NFT', 'DAO', 'Oracle', 'Bridge', 'DEX', 'Lending', 'Staking', 'Wallet', 'Token'];
    const tagOptions = ['soroban', 'stellar', 'defi', 'amm', 'lending', 'governance', 'token', 'nft', 'oracle', 'bridge'];
    const nameAdjectives = ['Swift', 'Quantum', 'Solar', 'Stellar', 'Bright', 'Nova', 'Cosmic', 'Nebula', 'Astral', 'Lunar'];
    const nameNouns = ['Swap', 'Vault', 'Pool', 'Bridge', 'Oracle', 'Token', 'Lend', 'Stake', 'DAO', 'Mint'];

    const nodes: GraphNode[] = [];
    for (let i = 0; i < nodeCount; i++) {
        const adj = nameAdjectives[Math.floor(Math.random() * nameAdjectives.length)];
        const noun = nameNouns[Math.floor(Math.random() * nameNouns.length)];
        const tagCount = 1 + Math.floor(Math.random() * 3);
        const tags: string[] = [];
        for (let t = 0; t < tagCount; t++) {
            const tag = tagOptions[Math.floor(Math.random() * tagOptions.length)];
            if (!tags.includes(tag)) tags.push(tag);
        }
        nodes.push({
            id: `demo-${i}`,
            contract_id: `C${Array.from({ length: 55 }, () => 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'[Math.floor(Math.random() * 32)]).join('')}`,
            name: `${adj}${noun}${i > 0 ? i : ''}`,
            network: networks[Math.floor(Math.random() * networks.length)],
            is_verified: Math.random() > 0.6,
            category: categories[Math.floor(Math.random() * categories.length)],
            tags,
        });
    }

    // Create edges — power-law distribution: some nodes get many dependents
    const edges: GraphEdge[] = [];
    const edgeCount = Math.min(nodeCount * 2, nodeCount * (nodeCount - 1) / 2);
    const edgeSet = new Set<string>();

    const pushEdge = (source: number, target: number, isCircular = false) => {
        if (source === target) return;
        const key = `${source}-${target}`;
        if (edgeSet.has(key)) return;
        edgeSet.add(key);
        const callFrequency = 1 + Math.floor(Math.random() * 250);
        const isEstimated = Math.random() > 0.6;
        edges.push({
            source: nodes[source].id,
            target: nodes[target].id,
            dependency_type: Math.random() > 0.7 ? 'imports' : 'calls',
            call_frequency: callFrequency,
            call_volume: callFrequency * (1 + Math.floor(Math.random() * 8)),
            is_estimated: isEstimated,
            is_circular: isCircular,
        });
    };

    for (let i = 0; i < edgeCount; i++) {
        // Bias towards lower-index nodes as targets to create hub nodes
        const sourceIdx = Math.floor(Math.random() * nodeCount);
        const targetIdx = Math.floor(Math.pow(Math.random(), 2) * nodeCount);
        pushEdge(sourceIdx, targetIdx);
    }

    // Inject a few explicit cycles to exercise cycle highlighting.
    const cycleGroups = Math.min(8, Math.floor(nodeCount / 10));
    for (let i = 0; i < cycleGroups; i++) {
        const a = i * 3;
        const b = a + 1;
        const c = a + 2;
        if (c >= nodeCount) break;
        pushEdge(a, b, true);
        pushEdge(b, c, true);
        pushEdge(c, a, true);
    }

    return { nodes, edges };
}

export function GraphContent() {
    const [networkFilter, setNetworkFilter] = useState<string>('');
    const [dependencyTypeFilter, setDependencyTypeFilter] = useState<string>('');
    const [showCyclesOnly, setShowCyclesOnly] = useState(false);
    const [minCallFrequency, setMinCallFrequency] = useState(0);
    const [searchQuery, setSearchQuery] = useState('');
    const [demoMode, setDemoMode] = useState(false);
    const [demoNodeCount, setDemoNodeCount] = useState(200);
    const [selectedNode, setSelectedNode] = useState<GraphNode | null>(null);
    const [searchMatchIndex, setSearchMatchIndex] = useState(0);
    const graphRef = useRef<DependencyGraphHandle | null>(null);
    const router = useRouter();
    const { logEvent } = useAnalytics();

    const { data: apiData, isLoading, error } = useQuery({
        queryKey: ['contract-graph', networkFilter],
        queryFn: () => api.getContractGraph(networkFilter || undefined),
        enabled: !demoMode,
    });

    useEffect(() => {
        if (!error) return;
        logEvent('error_event', {
            source: 'graph_page',
            message: 'Failed to load contract graph data',
            network_filter: networkFilter || 'all',
        });
    }, [error, networkFilter, logEvent]);

    useEffect(() => {
        if (!searchQuery.trim()) return;
        logEvent('search_performed', {
            keyword: searchQuery.trim(),
            source: 'graph_page',
            network_filter: networkFilter || 'all',
            demo_mode: demoMode,
        });
    }, [searchQuery, networkFilter, demoMode, logEvent]);

    const demoData = useMemo(
        () => (demoMode ? generateDemoData(demoNodeCount) : null),
        [demoMode, demoNodeCount]
    );

    // Apply client-side network filtering for demo mode
    const filteredDemoData = useMemo(() => {
        if (!demoData || !networkFilter) return demoData;
        const filteredNodes = demoData.nodes.filter((n) => n.network === networkFilter);
        const nodeIds = new Set(filteredNodes.map((n) => n.id));
        const filteredEdges = demoData.edges.filter(
            (e) => nodeIds.has(e.source) && nodeIds.has(e.target)
        );
        return { nodes: filteredNodes, edges: filteredEdges };
    }, [demoData, networkFilter]);

    const graphData = demoMode ? filteredDemoData : apiData;

    // Safe nodes/edges (API may return missing or non-array values)
    const rawNodes = useMemo(
        () => (graphData && Array.isArray(graphData.nodes) ? graphData.nodes : []),
        [graphData]
    );
    const rawEdges = useMemo(
        () => (graphData && Array.isArray(graphData.edges) ? graphData.edges : []),
        [graphData]
    );

    const filteredGraph = useMemo(() => {
        const edgeFiltered = rawEdges.filter((edge) => {
            if (dependencyTypeFilter && edge.dependency_type !== dependencyTypeFilter) return false;
            if (showCyclesOnly && !edge.is_circular) return false;
            if (minCallFrequency > 0 && (edge.call_frequency ?? 0) < minCallFrequency) return false;
            return true;
        });

        const usesEdgeFocusedFilter = showCyclesOnly || Boolean(dependencyTypeFilter) || minCallFrequency > 0;
        if (!usesEdgeFocusedFilter) {
            return { nodes: rawNodes, edges: edgeFiltered };
        }

        const nodeIds = new Set<string>();
        for (const edge of edgeFiltered) {
            nodeIds.add(edge.source);
            nodeIds.add(edge.target);
        }

        return {
            nodes: rawNodes.filter((node) => nodeIds.has(node.id)),
            edges: edgeFiltered,
        };
    }, [rawNodes, rawEdges, dependencyTypeFilter, showCyclesOnly, minCallFrequency]);

    const nodes = filteredGraph.nodes;
    const edges = filteredGraph.edges;

    // Compute dependent counts (how many nodes depend on this one = in-edges)
    const dependentCounts = useMemo(() => {
        const counts = new Map<string, number>();
        for (const edge of edges) {
            counts.set(edge.target, (counts.get(edge.target) || 0) + 1);
        }
        return counts;
    }, [edges]);

    // Compute dependency counts (how many nodes this one depends on = out-edges)
    const dependencyCounts = useMemo(() => {
        const counts = new Map<string, number>();
        for (const edge of edges) {
            counts.set(edge.source, (counts.get(edge.source) || 0) + 1);
        }
        return counts;
    }, [edges]);

    const criticalCount = useMemo(() => {
        let count = 0;
        dependentCounts.forEach((v) => { if (v >= 5) count++; });
        return count;
    }, [dependentCounts]);

    // Per-network node counts for the stats panel
    const networkCounts = useMemo(() => {
        const counts = { mainnet: 0, testnet: 0, futurenet: 0, other: 0 };
        for (const node of nodes) {
            const n = node.network?.toLowerCase() ?? "";
            if (n === "mainnet") counts.mainnet++;
            else if (n === "testnet") counts.testnet++;
            else if (n === "futurenet") counts.futurenet++;
            else counts.other++;
        }
        return counts;
    }, [nodes]);

    const handleNodeClick = useCallback((node: GraphNode | null) => {
        if (!node) {
            setSelectedNode(null);
            return;
        }

        router.push(`/contracts/${node.contract_id}`);
    }, [router]);

    // Search match navigation
    const searchMatches = useMemo(() => {
        if (!searchQuery || nodes.length === 0) return [];
        const q = searchQuery.toLowerCase();
        return nodes
            .filter((n) => n.name.toLowerCase().includes(q) || n.contract_id.toLowerCase().includes(q))
            .map((n) => n.id);
    }, [searchQuery, nodes]);

    // Reset match index when query or matches change
    useEffect(() => {
        setSearchMatchIndex(0);
    }, [searchQuery]);

    // Auto-focus on the active match
    useEffect(() => {
        if (searchMatches.length > 0 && graphRef.current) {
            graphRef.current.focusOnNode(searchMatches[searchMatchIndex] || searchMatches[0]);
        }
    }, [searchMatches, searchMatchIndex]);

    const handlePrevMatch = useCallback(() => {
        setSearchMatchIndex((i) => (i - 1 + searchMatches.length) % searchMatches.length);
    }, [searchMatches.length]);

    const handleNextMatch = useCallback(() => {
        setSearchMatchIndex((i) => (i + 1) % searchMatches.length);
    }, [searchMatches.length]);

    const cyclicEdgeCount = useMemo(
        () => edges.filter((edge) => edge.is_circular).length,
        [edges]
    );

    // Keyboard shortcuts
    useEffect(() => {
        const handleKeyDown = (e: KeyboardEvent) => {
            // Ignore when typing in inputs
            if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
            const g = graphRef.current;
            if (!g) return;
            switch (e.key) {
                case '=':
                case '+':
                    e.preventDefault();
                    g.zoomIn();
                    break;
                case '-':
                    e.preventDefault();
                    g.zoomOut();
                    break;
                case 'r':
                case 'R':
                    e.preventDefault();
                    g.resetZoom();
                    break;
                case 'ArrowUp':
                    e.preventDefault();
                    g.panUp();
                    break;
                case 'ArrowDown':
                    e.preventDefault();
                    g.panDown();
                    break;
                case 'ArrowLeft':
                    e.preventDefault();
                    g.panLeft();
                    break;
                case 'ArrowRight':
                    e.preventDefault();
                    g.panRight();
                    break;
                case 'Escape':
                    setSelectedNode(null);
                    break;
            }
        };
        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, []);

    if (!demoMode && isLoading) {
        return (
            <div className="flex items-center justify-center h-[calc(100vh-4rem)] bg-background">
                <div className="text-center">
                    <div className="inline-block w-10 h-10 border-4 border-primary border-t-transparent rounded-full animate-spin mb-4" />
                    <p className="text-muted-foreground text-sm">Loading graph data…</p>
                </div>
            </div>
        );
    }

    if (!demoMode && error) {
        return (
            <div className="relative h-[calc(100vh-4rem)] overflow-hidden bg-background">
                <div className="absolute inset-0 flex items-center justify-center z-20">
                    <div className="text-center gradient-border-card p-10 max-w-md">
                        <div className="w-14 h-14 rounded-2xl bg-amber-500/10 flex items-center justify-center mx-auto mb-4">
                            <AlertCircle className="w-7 h-7 text-amber-500" />
                        </div>
                        <h3 className="text-xl font-semibold text-foreground mb-2">Backend API unavailable</h3>
                        <p className="text-muted-foreground mb-6 text-sm leading-relaxed">
                            Could not load contract graph data. You can still explore the visualization using Demo Mode with synthetic data.
                        </p>
                        <button
                            onClick={() => setDemoMode(true)}
                            className="btn-glow px-6 py-2.5 bg-primary hover:brightness-110 text-primary-foreground rounded-lg font-medium transition-all inline-flex items-center gap-2"
                        >
                            <Sparkles className="w-4 h-4" />
                            Enable Demo Mode
                        </button>
                    </div>
                </div>
            </div>
        );
    }

    return (
        <div className="relative h-[calc(100vh-4rem)] overflow-hidden bg-background">
            {/* Graph Canvas */}
            <div className="w-full h-full bg-background relative">
                <DependencyGraph
                    ref={graphRef}
                    nodes={nodes}
                    edges={edges}
                    searchQuery={searchQuery}
                    dependentCounts={dependentCounts}
                    onNodeClick={handleNodeClick}
                    selectedNode={selectedNode}
                />
            </div>

            {/* Controls Overlay */}
            <GraphControls
                searchQuery={searchQuery}
                onSearchChange={setSearchQuery}
                networkFilter={networkFilter}
                onNetworkFilterChange={setNetworkFilter}
                dependencyTypeFilter={dependencyTypeFilter}
                onDependencyTypeFilterChange={setDependencyTypeFilter}
                showCyclesOnly={showCyclesOnly}
                onShowCyclesOnlyChange={setShowCyclesOnly}
                minCallFrequency={minCallFrequency}
                onMinCallFrequencyChange={setMinCallFrequency}
                demoMode={demoMode}
                onDemoModeChange={setDemoMode}
                demoNodeCount={demoNodeCount}
                onDemoNodeCountChange={setDemoNodeCount}
                totalNodes={nodes.length}
                totalEdges={edges.length}
                cyclicEdgeCount={cyclicEdgeCount}
                criticalCount={criticalCount}
                searchMatchCount={searchMatches.length}
                searchMatchIndex={searchMatchIndex}
                onPrevMatch={handlePrevMatch}
                onNextMatch={handleNextMatch}
                onZoomIn={() => graphRef.current?.zoomIn()}
                onZoomOut={() => graphRef.current?.zoomOut()}
                onResetZoom={() => graphRef.current?.resetZoom()}
                onExportSVG={() => graphRef.current?.exportSVG()}
                onExportPNG={() => graphRef.current?.exportPNG()}
                onPanUp={() => graphRef.current?.panUp()}
                onPanDown={() => graphRef.current?.panDown()}
                onPanLeft={() => graphRef.current?.panLeft()}
                onPanRight={() => graphRef.current?.panRight()}
                networkCounts={networkCounts}
            />

            {/* Selected Node Panel */}
            {selectedNode && (
                <div className="absolute bottom-4 left-4 z-30 w-80 bg-card/90 backdrop-blur-xl border border-border rounded-xl shadow-lg overflow-hidden">
                    {/* Header */}
                    <div className="p-4 pb-3">
                        <div className="flex items-start justify-between">
                            <div className="flex-1 min-w-0 pr-2">
                                <h3 className="font-semibold text-foreground text-base truncate">{selectedNode.name}</h3>
                                <p className="text-[10px] text-muted-foreground font-mono truncate mt-0.5">{selectedNode.contract_id}</p>
                            </div>
                            <button
                                onClick={() => setSelectedNode(null)}
                                className="text-muted-foreground hover:text-foreground transition-colors shrink-0 p-1 rounded hover:bg-accent focus-visible:ring-1 focus-visible:ring-primary focus:outline-none"
                                aria-label="Close panel"
                            >
                                <X className="w-4 h-4" />
                            </button>
                        </div>
                    </div>

                    {/* Stats row */}
                    <div className="grid grid-cols-3 gap-px bg-border">
                        <div className="bg-card p-2.5 text-center">
                            <div className="text-lg font-bold text-foreground">{dependentCounts.get(selectedNode.id) || 0}</div>
                            <div className="text-[10px] text-muted-foreground">Dependents</div>
                        </div>
                        <div className="bg-card p-2.5 text-center">
                            <div className="text-lg font-bold text-foreground">{dependencyCounts.get(selectedNode.id) || 0}</div>
                            <div className="text-[10px] text-muted-foreground">Dependencies</div>
                        </div>
                        <div className="bg-card p-2.5 text-center">
                            <div className={`text-sm font-bold ${selectedNode.is_verified ? 'text-green-500' : 'text-muted-foreground'}`}>
                                {selectedNode.is_verified ? '✓ Yes' : '—'}
                            </div>
                            <div className="text-[10px] text-muted-foreground">Verified</div>
                        </div>
                    </div>

                    {/* Details + Tags */}
                    <div className="p-4 pt-3 space-y-3">
                        <div className="space-y-1.5 text-sm">
                            <div className="flex justify-between">
                                <span className="text-muted-foreground">Network</span>
                                <span className={`font-medium px-2 py-0.5 rounded-full text-xs ${selectedNode.network === 'mainnet' ? 'text-green-600 bg-green-500/10' :
                                    selectedNode.network === 'testnet' ? 'text-blue-600 bg-blue-500/10' : 'text-purple-600 bg-purple-500/10'
                                    }`}>{selectedNode.network}</span>
                            </div>
                            {selectedNode.category && (
                                <div className="flex justify-between">
                                    <span className="text-muted-foreground">Type</span>
                                    <span className="text-foreground font-medium">{selectedNode.category}</span>
                                </div>
                            )}
                        </div>

                        {/* Tags */}
                        {selectedNode.tags.length > 0 && (
                            <div className="pt-2 border-t border-border">
                                <div className="flex flex-wrap gap-1">
                                    {selectedNode.tags.map((tag) => (
                                        <span key={tag} className="px-2 py-0.5 bg-primary/10 text-primary rounded-md text-[10px] font-medium">
                                            {tag}
                                        </span>
                                    ))}
                                </div>
                            </div>
                        )}

                        {/* Link */}
                        <a
                            href={`/contracts/${selectedNode.contract_id}`}
                            className="flex items-center justify-center gap-1.5 w-full py-2 bg-primary text-primary-foreground rounded-lg text-xs font-semibold btn-glow hover:brightness-110 transition-all focus-visible:ring-1 focus-visible:ring-primary focus:outline-none"
                        >
                            <ExternalLink className="w-3 h-3" />
                            View Contract Details
                        </a>
                    </div>
                </div>
            )}

            {/* Empty State */}
            {!demoMode && nodes.length === 0 && !isLoading && (
                <div className="absolute inset-0 flex items-center justify-center z-20">
                    <div className="text-center gradient-border-card p-10 max-w-md">
                        <div className="w-14 h-14 rounded-2xl bg-primary/10 flex items-center justify-center mx-auto mb-4">
                            <Sparkles className="w-7 h-7 text-primary" />
                        </div>
                        <h3 className="text-xl font-semibold text-foreground mb-2">No contracts yet</h3>
                        <p className="text-muted-foreground mb-6 text-sm leading-relaxed">
                            Publish some contracts or enable Demo Mode to explore the graph visualization.
                        </p>
                        <button
                            onClick={() => setDemoMode(true)}
                            className="btn-glow px-6 py-2.5 bg-primary hover:brightness-110 text-primary-foreground rounded-lg font-medium transition-all inline-flex items-center gap-2"
                        >
                            <Sparkles className="w-4 h-4" />
                            Enable Demo Mode
                        </button>
                    </div>
                </div>
            )}
        </div>
    );
}
