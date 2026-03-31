"use client";

import React, { useState, useMemo, useCallback } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import type { ContractVersion } from "@/lib/api";
import { diffLines } from "@/utils/comparison";
import type { DiffLine } from "@/utils/comparison";
import {
  Columns2,
  AlignLeft,
  Download,
  MessageSquare,
  X,
  ChevronDown,
  Plus,
  Minus,
  GitCompare,
} from "lucide-react";

// ── Types ─────────────────────────────────────────────────────────────────────

type ViewMode = "unified" | "side-by-side";

interface LineComment {
  id: string;
  lineKey: string; // `${lineNo}-${side}`
  text: string;
  createdAt: string;
}

// A row in the side-by-side table: context spans both columns;
// a change row may have content on one or both sides.
type SideBySideRow =
  | { kind: "context"; lineNo: number; value: string }
  | { kind: "change"; leftNo: number | null; leftValue: string | null; rightNo: number | null; rightValue: string | null };

// ── Rust syntax tokeniser ─────────────────────────────────────────────────────

const RUST_KW = new Set([
  "fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "use", "mod",
  "match", "if", "else", "for", "while", "loop", "return", "async", "await",
  "where", "crate", "Self", "self", "const", "static", "type", "move", "ref",
  "in", "as", "unsafe", "extern", "dyn", "Box", "Option", "Result", "Some",
  "None", "Ok", "Err", "true", "false",
]);

function tokenClass(token: string, inComment: boolean): string {
  if (inComment) return "text-emerald-500 dark:text-emerald-400";
  if (token.startsWith('"') || (token.startsWith("'") && token.length >= 3))
    return "text-amber-600 dark:text-amber-300";
  if (/^\d/.test(token)) return "text-purple-600 dark:text-purple-300";
  if (RUST_KW.has(token.replace(/[^A-Za-z0-9_]/g, "")))
    return "text-sky-600 dark:text-sky-300 font-semibold";
  if (/^[A-Z][A-Za-z0-9_]*$/.test(token)) return "text-teal-600 dark:text-teal-300";
  return "";
}

function HighlightedLine({ value }: { value: string }) {
  const tokens = value.split(/(\s+)/);
  const rendered = tokens.reduce<{ inComment: boolean; nodes: React.ReactNode[] }>(
    ({ inComment, nodes }, tok, i) => {
      const nowInComment = inComment || tok.includes("//");
      const cls = tokenClass(tok, nowInComment);
      const node = cls ? (
        <span key={i} className={cls}>
          {tok}
        </span>
      ) : (
        tok
      );
      return { inComment: nowInComment, nodes: [...nodes, node] };
    },
    { inComment: false, nodes: [] }
  );
  return <>{rendered.nodes}</>;
}

// ── Diff statistics ───────────────────────────────────────────────────────────

function calcStats(lines: DiffLine[]) {
  let added = 0;
  let removed = 0;
  for (const l of lines) {
    if (l.type === "add") added++;
    else if (l.type === "remove") removed++;
  }
  return { added, removed, changed: Math.min(added, removed) };
}

// ── Patch-file generation ─────────────────────────────────────────────────────

function buildPatch(lines: DiffLine[], fromLabel: string, toLabel: string): string {
  const hunkLines: string[] = [];
  const oldStart = 1;
  const newStart = 1;
  let oldCount = 0;
  let newCount = 0;

  for (const l of lines) {
    if (l.type === "context") {
      hunkLines.push(` ${l.value}`);
      oldCount++;
      newCount++;
    } else if (l.type === "remove") {
      hunkLines.push(`-${l.value}`);
      oldCount++;
    } else {
      hunkLines.push(`+${l.value}`);
      newCount++;
    }
  }

  const header = `--- a/${fromLabel}\n+++ b/${toLabel}\n@@ -${oldStart},${oldCount} +${newStart},${newCount} @@\n`;
  return header + hunkLines.join("\n") + "\n";
}

function downloadPatch(content: string, filename: string) {
  const blob = new Blob([content], { type: "text/plain" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

// ── Side-by-side converter ────────────────────────────────────────────────────

function toSideBySideRows(lines: DiffLine[]): SideBySideRow[] {
  const rows: SideBySideRow[] = [];
  let leftNo = 1;
  let rightNo = 1;
  let i = 0;

  while (i < lines.length) {
    const l = lines[i];

    if (l.type === "context") {
      rows.push({ kind: "context", lineNo: leftNo, value: l.value });
      leftNo++;
      rightNo++;
      i++;
      continue;
    }

    // Collect a run of removes and adds
    const removes: string[] = [];
    const adds: string[] = [];

    while (i < lines.length && lines[i].type === "remove") {
      removes.push(lines[i].value);
      i++;
    }
    while (i < lines.length && lines[i].type === "add") {
      adds.push(lines[i].value);
      i++;
    }

    const maxLen = Math.max(removes.length, adds.length);
    for (let j = 0; j < maxLen; j++) {
      const leftVal = removes[j] ?? null;
      const rightVal = adds[j] ?? null;
      rows.push({
        kind: "change",
        leftNo: leftVal !== null ? leftNo : null,
        leftValue: leftVal,
        rightNo: rightVal !== null ? rightNo : null,
        rightValue: rightVal,
      });
      if (leftVal !== null) leftNo++;
      if (rightVal !== null) rightNo++;
    }
  }

  return rows;
}

// ── Line comment widget ───────────────────────────────────────────────────────

interface CommentThreadProps {
  lineKey: string;
  comments: LineComment[];
  onAdd: (lineKey: string, text: string) => void;
  onClose: () => void;
}

function CommentThread({ lineKey, comments, onAdd, onClose }: CommentThreadProps) {
  const [draft, setDraft] = useState("");

  return (
    <div className="mx-2 mb-2 rounded-xl border border-primary/30 bg-card p-3 text-xs">
      <div className="flex items-center justify-between mb-2">
        <span className="font-semibold text-foreground">Comments</span>
        <button type="button" onClick={onClose} className="text-muted-foreground hover:text-foreground">
          <X size={14} />
        </button>
      </div>

      {comments.length === 0 && (
        <div className="mb-2 text-muted-foreground">No comments yet.</div>
      )}
      {comments.map((c) => (
        <div key={c.id} className="mb-2 rounded-lg bg-accent/30 p-2">
          <div className="text-foreground">{c.text}</div>
          <div className="mt-0.5 text-muted-foreground">
            {new Date(c.createdAt).toLocaleString()}
          </div>
        </div>
      ))}

      <div className="flex gap-2 mt-2">
        <textarea
          className="flex-1 resize-none rounded-lg border border-border bg-background px-2 py-1 text-foreground outline-none focus:border-primary/60"
          rows={2}
          placeholder="Add a comment…"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
        />
        <button
          type="button"
          disabled={!draft.trim()}
          onClick={() => {
            if (draft.trim()) {
              onAdd(lineKey, draft.trim());
              setDraft("");
            }
          }}
          className="self-end rounded-lg bg-primary/10 px-3 py-1 font-semibold text-primary hover:bg-primary/20 disabled:opacity-40"
        >
          Post
        </button>
      </div>
    </div>
  );
}

// ── Stats bar ─────────────────────────────────────────────────────────────────

function StatsBar({ stats }: { stats: { added: number; removed: number; changed: number } }) {
  return (
    <div className="flex items-center gap-4 text-xs">
      <span className="flex items-center gap-1 text-green-600 dark:text-green-400">
        <Plus size={13} />
        {stats.added} added
      </span>
      <span className="flex items-center gap-1 text-red-600 dark:text-red-400">
        <Minus size={13} />
        {stats.removed} removed
      </span>
      {stats.changed > 0 && (
        <span className="text-muted-foreground">{stats.changed} line(s) modified</span>
      )}
      {stats.added === 0 && stats.removed === 0 && (
        <span className="text-muted-foreground">No differences</span>
      )}
    </div>
  );
}

// ── Unified diff renderer ─────────────────────────────────────────────────────

interface UnifiedViewProps {
  lines: DiffLine[];
  comments: Record<string, LineComment[]>;
  openThread: string | null;
  onToggleThread: (key: string) => void;
  onAddComment: (lineKey: string, text: string) => void;
  onCloseThread: () => void;
}

function UnifiedView({
  lines,
  comments,
  openThread,
  onToggleThread,
  onAddComment,
  onCloseThread,
}: UnifiedViewProps) {
  let leftNo = 1;
  let rightNo = 1;

  return (
    <div className="overflow-x-auto">
      <table className="w-full border-collapse font-mono text-xs leading-5">
        <colgroup>
          <col className="w-10" />
          <col className="w-10" />
          <col className="w-5" />
          <col />
          <col className="w-6" />
        </colgroup>
        <tbody>
          {lines.map((l, idx) => {
            const isAdd = l.type === "add";
            const isRemove = l.type === "remove";

            const ln = isAdd ? rightNo : isRemove ? leftNo : leftNo;
            const lineKey = `${ln}-${isAdd ? "right" : "left"}`;

            const rowClass = isAdd
              ? "bg-green-500/10"
              : isRemove
              ? "bg-red-500/10"
              : "";

            const prefix = isAdd ? "+" : isRemove ? "-" : " ";
            const prefixClass = isAdd
              ? "text-green-600 dark:text-green-400 select-none"
              : isRemove
              ? "text-red-600 dark:text-red-400 select-none"
              : "text-muted-foreground select-none";
            const textClass = isAdd
              ? "text-green-700 dark:text-green-300"
              : isRemove
              ? "text-red-700 dark:text-red-300"
              : "text-foreground";

            const leftLabel = isAdd ? "" : String(leftNo);
            const rightLabel = isRemove ? "" : String(rightNo);

            if (l.type === "context") { leftNo++; rightNo++; }
            else if (l.type === "add") rightNo++;
            else leftNo++;

            const hasComments = (comments[lineKey]?.length ?? 0) > 0;

            return (
              <>
                <tr key={idx} className={`group ${rowClass} hover:brightness-95 dark:hover:brightness-110`}>
                  <td className="px-2 text-right text-muted-foreground select-none border-r border-border/40 tabular-nums">
                    {leftLabel}
                  </td>
                  <td className="px-2 text-right text-muted-foreground select-none border-r border-border/40 tabular-nums">
                    {rightLabel}
                  </td>
                  <td className={`px-1 text-center ${prefixClass}`}>{prefix}</td>
                  <td className={`py-0.5 pr-2 ${textClass}`}>
                    <HighlightedLine value={l.value} />
                  </td>
                  <td className="pl-1">
                    <button
                      type="button"
                      onClick={() => onToggleThread(lineKey)}
                      className={`opacity-0 group-hover:opacity-100 transition-opacity rounded p-0.5 hover:bg-primary/10 ${hasComments ? "!opacity-100 text-primary" : "text-muted-foreground"}`}
                      title="Comment on this line"
                    >
                      <MessageSquare size={12} />
                    </button>
                  </td>
                </tr>
                {openThread === lineKey && (
                  <tr key={`${idx}-comment`}>
                    <td colSpan={5}>
                      <CommentThread
                        lineKey={lineKey}
                        comments={comments[lineKey] ?? []}
                        onAdd={onAddComment}
                        onClose={onCloseThread}
                      />
                    </td>
                  </tr>
                )}
              </>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

// ── Side-by-side renderer ─────────────────────────────────────────────────────

interface SideBySideViewProps {
  rows: SideBySideRow[];
  comments: Record<string, LineComment[]>;
  openThread: string | null;
  onToggleThread: (key: string) => void;
  onAddComment: (lineKey: string, text: string) => void;
}

function SideBySideView({
  rows,
  comments,
  openThread,
  onToggleThread,
  onAddComment,
}: SideBySideViewProps) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full border-collapse font-mono text-xs leading-5 table-fixed">
        <colgroup>
          <col className="w-8" />
          <col className="w-[calc(50%-2.5rem)]" />
          <col className="w-5" />
          <col className="w-8" />
          <col className="w-[calc(50%-2.5rem)]" />
          <col className="w-5" />
        </colgroup>
        <thead>
          <tr className="border-b border-border text-muted-foreground text-xs">
            <th colSpan={3} className="py-1 px-2 text-left font-semibold">Before</th>
            <th colSpan={3} className="py-1 px-2 text-left font-semibold border-l border-border">After</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row, idx) => {
            if (row.kind === "context") {
              return (
                <tr key={idx} className="hover:brightness-95 dark:hover:brightness-110">
                  <td className="px-2 text-right text-muted-foreground select-none border-r border-border/40 tabular-nums">
                    {row.lineNo}
                  </td>
                  <td className="py-0.5 pr-2 text-foreground" colSpan={2}>
                    <HighlightedLine value={row.value} />
                  </td>
                  <td className="px-2 text-right text-muted-foreground select-none border-r border-border/40 tabular-nums border-l border-border/40">
                    {row.lineNo}
                  </td>
                  <td className="py-0.5 pr-2 text-foreground" colSpan={2}>
                    <HighlightedLine value={row.value} />
                  </td>
                </tr>
              );
            }

            const leftKey = row.leftNo !== null ? `${row.leftNo}-left` : null;
            const rightKey = row.rightNo !== null ? `${row.rightNo}-right` : null;
            const leftHasComments = leftKey ? (comments[leftKey]?.length ?? 0) > 0 : false;
            const rightHasComments = rightKey ? (comments[rightKey]?.length ?? 0) > 0 : false;

            return (
              <>
                <tr key={idx} className="group">
                  {/* Left (remove) */}
                  <td className="px-2 text-right text-muted-foreground select-none border-r border-border/40 tabular-nums bg-red-500/10">
                    {row.leftNo ?? ""}
                  </td>
                  <td className={`py-0.5 pr-1 ${row.leftValue !== null ? "bg-red-500/10 text-red-700 dark:text-red-300" : "bg-background"}`}>
                    {row.leftValue !== null && (
                      <>
                        <span className="text-red-500 select-none mr-1">-</span>
                        <HighlightedLine value={row.leftValue} />
                      </>
                    )}
                  </td>
                  <td className={`pl-1 ${row.leftValue !== null ? "bg-red-500/10" : "bg-background"}`}>
                    {leftKey && (
                      <button
                        type="button"
                        onClick={() => onToggleThread(leftKey)}
                        className={`opacity-0 group-hover:opacity-100 transition-opacity rounded p-0.5 hover:bg-primary/10 ${leftHasComments ? "!opacity-100 text-primary" : "text-muted-foreground"}`}
                        title="Comment"
                      >
                        <MessageSquare size={12} />
                      </button>
                    )}
                  </td>

                  {/* Right (add) */}
                  <td className="px-2 text-right text-muted-foreground select-none border-r border-border/40 tabular-nums bg-green-500/10 border-l border-border/40">
                    {row.rightNo ?? ""}
                  </td>
                  <td className={`py-0.5 pr-1 ${row.rightValue !== null ? "bg-green-500/10 text-green-700 dark:text-green-300" : "bg-background"}`}>
                    {row.rightValue !== null && (
                      <>
                        <span className="text-green-500 select-none mr-1">+</span>
                        <HighlightedLine value={row.rightValue} />
                      </>
                    )}
                  </td>
                  <td className={`pl-1 ${row.rightValue !== null ? "bg-green-500/10" : "bg-background"}`}>
                    {rightKey && (
                      <button
                        type="button"
                        onClick={() => onToggleThread(rightKey)}
                        className={`opacity-0 group-hover:opacity-100 transition-opacity rounded p-0.5 hover:bg-primary/10 ${rightHasComments ? "!opacity-100 text-primary" : "text-muted-foreground"}`}
                        title="Comment"
                      >
                        <MessageSquare size={12} />
                      </button>
                    )}
                  </td>
                </tr>
                {(openThread === leftKey || openThread === rightKey) && (
                  <tr key={`${idx}-comment`}>
                    <td colSpan={6}>
                      <CommentThread
                        lineKey={openThread!}
                        comments={comments[openThread!] ?? []}
                        onAdd={onAddComment}
                        onClose={() => onToggleThread(openThread!)}
                      />
                    </td>
                  </tr>
                )}
              </>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

// ── Version selector ──────────────────────────────────────────────────────────

interface VersionSelectProps {
  label: string;
  versions: ContractVersion[];
  value: string;
  onChange: (v: string) => void;
  exclude?: string;
}

function VersionSelect({ label, versions, value, onChange, exclude }: VersionSelectProps) {
  const opts = versions.filter((v) => v.version !== exclude);
  return (
    <div className="flex flex-col gap-1">
      <label className="text-xs font-semibold text-muted-foreground">{label}</label>
      <div className="relative">
        <select
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className="w-full appearance-none rounded-xl border border-border bg-background pl-3 pr-8 py-2 text-sm text-foreground focus:border-primary/60 focus:outline-none"
        >
          {opts.map((v) => (
            <option key={v.id} value={v.version}>
              {v.version}
              {v.commit_hash ? ` (${v.commit_hash.slice(0, 7)})` : ""}
            </option>
          ))}
        </select>
        <ChevronDown
          size={14}
          className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground pointer-events-none"
        />
      </div>
    </div>
  );
}

// ── Main component ────────────────────────────────────────────────────────────

interface ContractDiffViewerProps {
  contractId: string;
  contractName?: string;
}

export default function ContractDiffViewer({
  contractId,
  contractName,
}: ContractDiffViewerProps) {
  const [viewMode, setViewMode] = useState<ViewMode>("unified");
  const [fromVersion, setFromVersion] = useState<string>("");
  const [toVersion, setToVersion] = useState<string>("");
  const [comments, setComments] = useState<Record<string, LineComment[]>>({});
  const [openThread, setOpenThread] = useState<string | null>(null);

  // Fetch versions list
  const versionsQuery = useQuery({
    queryKey: ["contract-versions", contractId],
    queryFn: () => api.getContractVersions(contractId),
    enabled: !!contractId,
  });

  const versions = useMemo(() => versionsQuery.data ?? [], [versionsQuery.data]);

  // Derive effective selections — empty state means "use default from list"
  const effectiveFrom = useMemo(() => {
    if (fromVersion) return fromVersion;
    if (versions.length >= 2) return versions[versions.length - 2].version;
    return versions[0]?.version ?? "";
  }, [fromVersion, versions]);

  const effectiveTo = useMemo(() => {
    if (toVersion) return toVersion;
    return versions[versions.length - 1]?.version ?? "";
  }, [toVersion, versions]);

  const fromMeta = versions.find((v) => v.version === effectiveFrom) ?? null;
  const toMeta = versions.find((v) => v.version === effectiveTo) ?? null;

  // Fetch source code for each selected version
  const sourceQuery = useCallback(
    (meta: ContractVersion | null) => {
      if (!meta?.source_url) return Promise.resolve<string>("");
      const rawUrl = meta.source_url.replace(
        /^https:\/\/github\.com\/([^/]+)\/([^/]+)\/blob\/([^/]+)\/(.+)$/,
        "https://raw.githubusercontent.com/$1/$2/$3/$4"
      );
      return fetch(rawUrl)
        .then((r) => (r.ok ? r.text() : ""))
        .catch(() => "");
    },
    []
  );

  const fromSourceQuery = useQuery({
    queryKey: ["diff-source", contractId, effectiveFrom],
    queryFn: () => sourceQuery(fromMeta),
    enabled: !!fromMeta,
  });

  const toSourceQuery = useQuery({
    queryKey: ["diff-source", contractId, effectiveTo],
    queryFn: () => sourceQuery(toMeta),
    enabled: !!toMeta,
  });

  // Fall back to stub source when real source is unavailable
  const fromSource = fromSourceQuery.data ?? fromMeta?.wasm_hash
    ? `// ${contractName ?? contractId} — version ${effectiveFrom}\n// Source not available (wasm_hash: ${fromMeta?.wasm_hash ?? ""})\n`
    : "";
  const toSource = toSourceQuery.data ?? toMeta?.wasm_hash
    ? `// ${contractName ?? contractId} — version ${effectiveTo}\n// Source not available (wasm_hash: ${toMeta?.wasm_hash ?? ""})\n`
    : "";

  const diffResult = useMemo(() => {
    if (!effectiveFrom || !effectiveTo || effectiveFrom === effectiveTo) return [];
    return diffLines(fromSource, toSource);
  }, [fromSource, toSource, effectiveFrom, effectiveTo]);

  const stats = useMemo(() => calcStats(diffResult), [diffResult]);
  const sideBySideRows = useMemo(
    () => (viewMode === "side-by-side" ? toSideBySideRows(diffResult) : []),
    [diffResult, viewMode]
  );

  const handleToggleThread = useCallback(
    (key: string) => setOpenThread((prev) => (prev === key ? null : key)),
    []
  );

  const handleAddComment = useCallback((lineKey: string, text: string) => {
    setComments((prev) => ({
      ...prev,
      [lineKey]: [
        ...(prev[lineKey] ?? []),
        {
          id: `${Date.now()}-${Math.random()}`,
          lineKey,
          text,
          createdAt: new Date().toISOString(),
        },
      ],
    }));
  }, []);

  const handleCloseThread = useCallback(() => setOpenThread(null), []);

  const handleDownloadPatch = useCallback(() => {
    if (diffResult.length === 0) return;
    const patch = buildPatch(
      diffResult,
      `${contractName ?? contractId}@${effectiveFrom}`,
      `${contractName ?? contractId}@${effectiveTo}`
    );
    downloadPatch(
      patch,
      `${contractName ?? contractId}-${effectiveFrom}-to-${effectiveTo}.patch`
    );
  }, [diffResult, contractId, contractName, effectiveFrom, effectiveTo]);

  const totalComments = useMemo(
    () => Object.values(comments).reduce((s, arr) => s + arr.length, 0),
    [comments]
  );

  // ── Render ──────────────────────────────────────────────────────────────────

  if (versionsQuery.isPending) {
    return (
      <div className="rounded-2xl border border-border bg-card p-6 animate-pulse">
        <div className="h-5 w-40 rounded bg-border mb-3" />
        <div className="h-3 w-64 rounded bg-border" />
      </div>
    );
  }

  if (versionsQuery.isError || versions.length === 0) {
    return (
      <div className="rounded-2xl border border-border bg-card p-6">
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <GitCompare size={16} />
          {versions.length === 0
            ? "No versions available to diff."
            : "Failed to load contract versions."}
        </div>
      </div>
    );
  }

  const sameVersion = effectiveFrom === effectiveTo;

  return (
    <div className="flex flex-col gap-4">
      {/* Header controls */}
      <div className="rounded-2xl border border-border bg-card p-4">
        <div className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
          <div className="flex flex-col gap-1">
            <div className="flex items-center gap-2">
              <GitCompare size={16} className="text-primary" />
              <span className="text-sm font-semibold text-foreground">Code Diff</span>
              {totalComments > 0 && (
                <span className="flex items-center gap-1 rounded-full bg-primary/10 px-2 py-0.5 text-xs font-semibold text-primary">
                  <MessageSquare size={11} />
                  {totalComments}
                </span>
              )}
            </div>
            <div className="text-xs text-muted-foreground">
              Compare source code between two versions of this contract.
            </div>
          </div>

          {/* View mode toggle */}
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => setViewMode("unified")}
              className={`flex items-center gap-1.5 rounded-xl border px-3 py-1.5 text-xs font-semibold transition-colors ${
                viewMode === "unified"
                  ? "border-primary/30 bg-primary/10 text-primary"
                  : "border-border bg-background text-muted-foreground hover:text-foreground hover:bg-accent"
              }`}
            >
              <AlignLeft size={13} />
              Unified
            </button>
            <button
              type="button"
              onClick={() => setViewMode("side-by-side")}
              className={`flex items-center gap-1.5 rounded-xl border px-3 py-1.5 text-xs font-semibold transition-colors ${
                viewMode === "side-by-side"
                  ? "border-primary/30 bg-primary/10 text-primary"
                  : "border-border bg-background text-muted-foreground hover:text-foreground hover:bg-accent"
              }`}
            >
              <Columns2 size={13} />
              Side by side
            </button>
          </div>
        </div>

        {/* Version selectors */}
        <div className="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-2 md:grid-cols-4">
          <div className="col-span-2 md:col-span-1">
            <VersionSelect
              label="From (base)"
              versions={versions}
              value={effectiveFrom}
              onChange={setFromVersion}
              exclude={effectiveTo}
            />
          </div>
          <div className="col-span-2 md:col-span-1">
            <VersionSelect
              label="To (compare)"
              versions={versions}
              value={effectiveTo}
              onChange={setToVersion}
              exclude={effectiveFrom}
            />
          </div>
          <div className="col-span-2 flex items-end gap-2">
            <StatsBar stats={stats} />
            <button
              type="button"
              disabled={diffResult.length === 0}
              onClick={handleDownloadPatch}
              className="ml-auto flex items-center gap-1.5 rounded-xl border border-border bg-background px-3 py-1.5 text-xs font-semibold text-muted-foreground hover:text-foreground hover:bg-accent disabled:opacity-40 transition-colors"
              title="Download .patch file"
            >
              <Download size={13} />
              Download patch
            </button>
          </div>
        </div>
      </div>

      {/* Diff body */}
      <div className="rounded-2xl border border-border bg-card overflow-hidden">
        {sameVersion ? (
          <div className="p-6 text-sm text-muted-foreground">
            Select two different versions to compare.
          </div>
        ) : fromSourceQuery.isPending || toSourceQuery.isPending ? (
          <div className="p-6 animate-pulse space-y-2">
            {Array.from({ length: 6 }).map((_, i) => (
              <div key={i} className="h-3 rounded bg-border" style={{ width: `${60 + (i % 3) * 15}%` }} />
            ))}
          </div>
        ) : diffResult.length === 0 ? (
          <div className="p-6 text-sm text-muted-foreground">
            No differences found between these versions.
          </div>
        ) : viewMode === "unified" ? (
          <UnifiedView
            lines={diffResult}
            comments={comments}
            openThread={openThread}
            onToggleThread={handleToggleThread}
            onAddComment={handleAddComment}
            onCloseThread={handleCloseThread}
          />
        ) : (
          <SideBySideView
            rows={sideBySideRows}
            comments={comments}
            openThread={openThread}
            onToggleThread={handleToggleThread}
            onAddComment={handleAddComment}
          />
        )}
      </div>
    </div>
  );
}
