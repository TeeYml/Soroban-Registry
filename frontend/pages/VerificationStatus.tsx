'use client';

import React, { useEffect, useRef, useState } from 'react';
import Link from 'next/link';
import { useSearchParams } from 'next/navigation';
import Navbar from '@/components/Navbar';
import StatusTracker from '@/components/verification/StatusTracker';
import VerificationBadge from '@/components/verification/VerificationBadge';
import VerificationSummary from '@/components/verification/VerificationSummary';
import { useToast } from '@/hooks/useToast';

export const dynamic = 'force-dynamic';
import {
  getVerificationStatus,
  simulateStatusProgression,
  subscribeToVerificationStatusChanges,
} from '@/services/mockVerificationService';
import type { VerificationRequest, VerificationStatus } from '@/types/verification';

export default function VerificationStatusPage() {
  const params = useSearchParams();
  const id = params?.get('id') || '';
  const { showInfo, showSuccess, showWarning, showError } = useToast();

  const [request, setRequest] = useState<VerificationRequest | null>(null);
  const [loading, setLoading] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const lastStatusRef = useRef<VerificationStatus | null>(null);

  useEffect(() => {
    if (!id) return;
    const kickoff = window.setTimeout(() => {
      setLoading(true);
      setErrorMsg(null);
      setRequest(null);
    }, 0);
    getVerificationStatus({ id })
      .then((res) => {
        setRequest(res.request);
        lastStatusRef.current = res.request.status;
      })
      .catch((err: unknown) => {
        setErrorMsg(err instanceof Error ? err.message : 'Failed to load status');
      })
      .finally(() => setLoading(false));
    return () => window.clearTimeout(kickoff);
  }, [id]);

  useEffect(() => {
    if (!id) return;
    const stop = simulateStatusProgression({ id });

    const unsub = subscribeToVerificationStatusChanges(async (evt) => {
      if (evt.id !== id) return;
      try {
        const res = await getVerificationStatus({ id });
        setRequest(res.request);
        const prev = lastStatusRef.current;
        const next = res.request.status;
        if (prev && prev !== next) {
          if (next === 'under_review') showInfo('Verification moved to under review.');
          if (next === 'approved') showSuccess('Verification approved. Badge is now visible.');
          if (next === 'rejected') showWarning('Verification rejected. Check details and resubmit if needed.');
        }
        lastStatusRef.current = next;
      } catch (err: unknown) {
        showError(err instanceof Error ? err.message : 'Failed to refresh status');
      }
    });

    return () => {
      stop();
      unsub();
    };
  }, [id, showError, showInfo, showSuccess, showWarning]);

  return (
    <div className="flex flex-col min-h-screen bg-background">
      <Navbar />
      <div className="max-w-5xl mx-auto py-8 px-4 sm:px-6 lg:px-8 w-full flex-grow">
        <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
          <div>
            <h1 className="text-2xl sm:text-3xl font-bold text-foreground">Verification Status</h1>
            <p className="text-sm text-muted-foreground mt-1">Track verification progress and review submitted information.</p>
          </div>
          <div className="flex items-center gap-2">
            <Link
              href="/verify-contract"
              className="px-4 py-2 rounded-lg border border-border bg-background text-foreground font-medium hover:bg-accent transition-colors"
            >
              New submission
            </Link>
          </div>
        </div>

        {!id ? (
          <div className="mt-6 rounded-2xl border border-border bg-card p-6">
            <p className="text-sm text-muted-foreground">Missing verification id. Start a new verification submission.</p>
            <div className="mt-4">
              <Link href="/verify-contract" className="px-6 py-2.5 rounded-lg btn-glow text-primary-foreground font-medium inline-flex">
                Verify a contract
              </Link>
            </div>
          </div>
        ) : loading ? (
          <div className="mt-6 rounded-2xl border border-border bg-card p-6">
            <p className="text-sm text-muted-foreground">Loading verification status…</p>
          </div>
        ) : errorMsg ? (
          <div className="mt-6 rounded-2xl border border-red-500/20 bg-red-500/10 p-6">
            <p className="text-sm text-red-600">{errorMsg}</p>
          </div>
        ) : request ? (
          <div className="mt-6 grid grid-cols-1 lg:grid-cols-3 gap-4">
            <div className="lg:col-span-1 space-y-4">
              <div className="rounded-2xl border border-border bg-card p-4">
                <p className="text-xs text-muted-foreground uppercase tracking-wide font-semibold">Current status</p>
                <div className="mt-2 flex items-center justify-between gap-3">
                  <p className="text-lg font-semibold text-foreground">{request.status.replaceAll('_', ' ')}</p>
                  <VerificationBadge status={request.status} size="md" />
                </div>
                {request.status === 'approved' && (
                  <p className="text-xs text-muted-foreground mt-2">This contract is now verified and will show the badge in listings.</p>
                )}
              </div>

              <StatusTracker status={request.status} history={request.statusHistory} />
            </div>

            <div className="lg:col-span-2 space-y-4">
              <VerificationSummary draft={request.submission} documents={request.submission.documents} status={request.status} />
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
