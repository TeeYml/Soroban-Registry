'use client';

import React from 'react';
import { CheckCircle2, ShieldAlert, ShieldCheck, ShieldX } from 'lucide-react';
import type { VerificationStatus } from '@/types/verification';

function getBadgeConfig(status: VerificationStatus): {
  label: string;
  className: string;
  Icon: React.ComponentType<{ className?: string }>;
} {
  switch (status) {
    case 'approved':
      return {
        label: 'Verified',
        className: 'bg-green-500/10 text-green-500 border-green-500/20',
        Icon: CheckCircle2,
      };
    case 'under_review':
      return {
        label: 'Under review',
        className: 'bg-blue-500/10 text-blue-500 border-blue-500/20',
        Icon: ShieldCheck,
      };
    case 'rejected':
      return {
        label: 'Rejected',
        className: 'bg-red-500/10 text-red-500 border-red-500/20',
        Icon: ShieldX,
      };
    case 'submitted':
      return {
        label: 'Submitted',
        className: 'bg-yellow-500/10 text-yellow-600 border-yellow-500/20',
        Icon: ShieldAlert,
      };
    default:
      return {
        label: 'Draft',
        className: 'bg-muted text-muted-foreground border-border',
        Icon: ShieldAlert,
      };
  }
}

export default function VerificationBadge(props: { status: VerificationStatus; size?: 'sm' | 'md' }) {
  const { status, size = 'sm' } = props;
  const cfg = getBadgeConfig(status);

  const iconSize = size === 'md' ? 'w-4 h-4' : 'w-3 h-3';
  const textSize = size === 'md' ? 'text-xs' : 'text-[10px]';
  const padding = size === 'md' ? 'px-2.5 py-1' : 'px-2 py-0.5';

  return (
    <span
      className={`inline-flex items-center gap-1 rounded-full border ${padding} ${textSize} font-semibold uppercase tracking-wide ${cfg.className}`}
    >
      <cfg.Icon className={iconSize} />
      {cfg.label}
    </span>
  );
}

