"use client";

import React, { useState } from "react";
import {
  api,
  ReleaseNotesResponse,
  ReleaseNotesStatus,
} from "@/lib/api";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";

interface ReleaseNotesProps {
  contractId: string;
}

export default function ReleaseNotesPanel({ contractId }: ReleaseNotesProps) {
  const queryClient = useQueryClient();
  const [editingVersion, setEditingVersion] = useState<string | null>(null);
  const [editText, setEditText] = useState("");
  const [generateVersion, setGenerateVersion] = useState("");
  const [showGenerateForm, setShowGenerateForm] = useState(false);

  // Fetch all release notes for the contract
  const {
    data: releaseNotes,
    isLoading,
    error,
  } = useQuery({
    queryKey: ["release-notes", contractId],
    queryFn: () => api.listReleaseNotes(contractId),
  });

  // Generate mutation
  const generateMutation = useMutation({
    mutationFn: (version: string) =>
      api.generateReleaseNotes(contractId, { version }),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: ["release-notes", contractId],
      });
      setShowGenerateForm(false);
      setGenerateVersion("");
    },
  });

  // Update mutation
  const updateMutation = useMutation({
    mutationFn: ({ version, text }: { version: string; text: string }) =>
      api.updateReleaseNotes(contractId, version, { notes_text: text }),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: ["release-notes", contractId],
      });
      setEditingVersion(null);
      setEditText("");
    },
  });

  // Publish mutation
  const publishMutation = useMutation({
    mutationFn: (version: string) =>
      api.publishReleaseNotes(contractId, version),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: ["release-notes", contractId],
      });
    },
  });

  if (isLoading) {
    return (
      <div className="animate-pulse space-y-3">
        <div className="h-6 bg-gray-200 dark:bg-gray-700 rounded w-1/3" />
        <div className="h-4 bg-gray-200 dark:bg-gray-700 rounded w-2/3" />
        <div className="h-4 bg-gray-200 dark:bg-gray-700 rounded w-1/2" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="text-red-500 text-sm">
        Failed to load release notes.
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
          Release Notes
        </h3>
        <button
          onClick={() => setShowGenerateForm(!showGenerateForm)}
          className="px-3 py-1.5 text-sm font-medium rounded-md bg-blue-600 text-white hover:bg-blue-700 transition-colors"
        >
          {showGenerateForm ? "Cancel" : "Generate Notes"}
        </button>
      </div>

      {/* Generate form */}
      {showGenerateForm && (
        <div className="p-4 border border-blue-200 dark:border-blue-800 rounded-lg bg-blue-50 dark:bg-blue-900/20 space-y-3">
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
            Version (semver)
          </label>
          <input
            type="text"
            value={generateVersion}
            onChange={(e) => setGenerateVersion(e.target.value)}
            placeholder="e.g. 1.2.0"
            className="w-full px-3 py-2 rounded-md border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm"
          />
          <button
            onClick={() => generateMutation.mutate(generateVersion)}
            disabled={!generateVersion || generateMutation.isPending}
            className="px-4 py-2 text-sm font-medium rounded-md bg-green-600 text-white hover:bg-green-700 disabled:opacity-50 transition-colors"
          >
            {generateMutation.isPending
              ? "Generating..."
              : "Auto-Generate"}
          </button>
          {generateMutation.isError && (
            <p className="text-red-500 text-sm">
              {(generateMutation.error as Error).message}
            </p>
          )}
        </div>
      )}

      {/* Release notes list */}
      {(!releaseNotes || releaseNotes.length === 0) && (
        <p className="text-sm text-gray-500 dark:text-gray-400">
          No release notes generated yet. Click &quot;Generate Notes&quot; to
          auto-generate from code diffs.
        </p>
      )}

      {releaseNotes?.map((rn) => (
        <ReleaseNotesCard
          key={rn.id}
          notes={rn}
          isEditing={editingVersion === rn.version}
          editText={editingVersion === rn.version ? editText : ""}
          onStartEdit={() => {
            setEditingVersion(rn.version);
            setEditText(rn.notes_text);
          }}
          onCancelEdit={() => {
            setEditingVersion(null);
            setEditText("");
          }}
          onChangeEditText={setEditText}
          onSaveEdit={() =>
            updateMutation.mutate({
              version: rn.version,
              text: editText,
            })
          }
          onPublish={() => publishMutation.mutate(rn.version)}
          isSaving={updateMutation.isPending}
          isPublishing={publishMutation.isPending}
        />
      ))}
    </div>
  );
}


interface ReleaseNotesCardProps {
  notes: ReleaseNotesResponse;
  isEditing: boolean;
  editText: string;
  onStartEdit: () => void;
  onCancelEdit: () => void;
  onChangeEditText: (text: string) => void;
  onSaveEdit: () => void;
  onPublish: () => void;
  isSaving: boolean;
  isPublishing: boolean;
}

function ReleaseNotesCard({
  notes,
  isEditing,
  editText,
  onStartEdit,
  onCancelEdit,
  onChangeEditText,
  onSaveEdit,
  onPublish,
  isSaving,
  isPublishing,
}: ReleaseNotesCardProps) {
  const isDraft = notes.status === "draft";

  return (
    <div className="border border-gray-200 dark:border-gray-700 rounded-lg overflow-hidden">
      {/* Card header */}
      <div className="flex items-center justify-between px-4 py-3 bg-gray-50 dark:bg-gray-800">
        <div className="flex items-center gap-3">
          <span className="font-semibold text-gray-900 dark:text-gray-100">
            v{notes.version}
          </span>
          <StatusBadge status={notes.status} />
          {notes.diff_summary.has_breaking_changes && (
            <span className="px-2 py-0.5 text-xs font-bold rounded bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-400">
              BREAKING
            </span>
          )}
          {notes.previous_version && (
            <span className="text-xs text-gray-500 dark:text-gray-400">
              (from v{notes.previous_version})
            </span>
          )}
        </div>
        <div className="flex gap-2">
          {isDraft && !isEditing && (
            <>
              <button
                onClick={onStartEdit}
                className="px-2.5 py-1 text-xs font-medium rounded bg-gray-200 dark:bg-gray-600 text-gray-700 dark:text-gray-200 hover:bg-gray-300 dark:hover:bg-gray-500 transition-colors"
              >
                Edit
              </button>
              <button
                onClick={onPublish}
                disabled={isPublishing}
                className="px-2.5 py-1 text-xs font-medium rounded bg-green-600 text-white hover:bg-green-700 disabled:opacity-50 transition-colors"
              >
                {isPublishing ? "Publishing..." : "Publish"}
              </button>
            </>
          )}
        </div>
      </div>

      {/* Diff summary */}
      <DiffSummaryBar diff={notes.diff_summary} />

      {/* Notes body */}
      <div className="p-4">
        {isEditing ? (
          <div className="space-y-3">
            <textarea
              value={editText}
              onChange={(e) => onChangeEditText(e.target.value)}
              rows={16}
              className="w-full px-3 py-2 rounded-md border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm font-mono"
            />
            <div className="flex gap-2">
              <button
                onClick={onSaveEdit}
                disabled={isSaving}
                className="px-3 py-1.5 text-sm font-medium rounded-md bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-50 transition-colors"
              >
                {isSaving ? "Saving..." : "Save Draft"}
              </button>
              <button
                onClick={onCancelEdit}
                className="px-3 py-1.5 text-sm font-medium rounded-md bg-gray-200 dark:bg-gray-600 text-gray-700 dark:text-gray-200 hover:bg-gray-300 dark:hover:bg-gray-500 transition-colors"
              >
                Cancel
              </button>
            </div>
          </div>
        ) : (
          <pre className="whitespace-pre-wrap text-sm text-gray-700 dark:text-gray-300 font-mono leading-relaxed">
            {notes.notes_text}
          </pre>
        )}
      </div>

      {/* Footer metadata */}
      <div className="px-4 py-2 bg-gray-50 dark:bg-gray-800 border-t border-gray-200 dark:border-gray-700 text-xs text-gray-500 dark:text-gray-400 flex gap-4">
        <span>Generated by: {notes.generated_by}</span>
        <span>Created: {new Date(notes.created_at).toLocaleDateString()}</span>
        {notes.published_at && (
          <span>
            Published: {new Date(notes.published_at).toLocaleDateString()}
          </span>
        )}
      </div>
    </div>
  );
}

function StatusBadge({ status }: { status: ReleaseNotesStatus }) {
  if (status === "published") {
    return (
      <span className="px-2 py-0.5 text-xs font-medium rounded-full bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-400">
        Published
      </span>
    );
  }
  return (
    <span className="px-2 py-0.5 text-xs font-medium rounded-full bg-yellow-100 text-yellow-700 dark:bg-yellow-900/40 dark:text-yellow-400">
      Draft
    </span>
  );
}

function DiffSummaryBar({
  diff,
}: {
  diff: ReleaseNotesResponse["diff_summary"];
}) {
  const added = diff.function_changes.filter(
    (c) => c.change_type === "added"
  ).length;
  const removed = diff.function_changes.filter(
    (c) => c.change_type === "removed"
  ).length;
  const modified = diff.function_changes.filter(
    (c) => c.change_type === "modified"
  ).length;

  return (
    <div className="flex flex-wrap gap-4 px-4 py-2 text-xs text-gray-600 dark:text-gray-400 border-b border-gray-100 dark:border-gray-700">
      <span>
        {diff.files_changed} file{diff.files_changed !== 1 ? "s" : ""} changed
      </span>
      <span className="text-green-600 dark:text-green-400">
        +{diff.lines_added}
      </span>
      <span className="text-red-500 dark:text-red-400">
        -{diff.lines_removed}
      </span>
      {added > 0 && (
        <span className="text-green-600 dark:text-green-400">
          {added} fn added
        </span>
      )}
      {removed > 0 && (
        <span className="text-red-500 dark:text-red-400">
          {removed} fn removed
        </span>
      )}
      {modified > 0 && (
        <span className="text-yellow-600 dark:text-yellow-400">
          {modified} fn modified
        </span>
      )}
    </div>
  );
}
