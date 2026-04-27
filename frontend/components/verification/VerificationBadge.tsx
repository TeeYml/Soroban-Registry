'use client';

import React from 'react';
import { CheckCircle2, ShieldAlert, ShieldCheck, ShieldX, Info } from 'lucide-react';
import type { VerificationStatus, VerificationLevel } from '@/types/verification';
import { useTranslation } from '@/lib/i18n/client';

function getBadgeConfig(status: VerificationStatus, level?: VerificationLevel): {
  label: string;
  className: string;
  Icon: React.ComponentType<{ className?: string }>;
  tooltip: string;
} {
  switch (status) {
    case 'approved':
      const levelLabel = level ? ` (${level.charAt(0).toUpperCase() + level.slice(1)})` : '';
      return {
        label: `Verified${levelLabel}`,
        className: 'bg-green-500/10 text-green-600 border-green-500/20',
        Icon: CheckCircle2,
        tooltip: level === 'advanced' ? 'Advanced Verification: Fully audited code with formal verification' :
                 level === 'intermediate' ? 'Intermediate Verification: Audited code' :
                 'Basic Verification: Automated checks passed',
      };
    case 'under_review':
    case 'submitted':
      return {
        label: 'Pending',
        className: 'bg-yellow-500/10 text-yellow-600 border-yellow-500/20',
        Icon: ShieldCheck,
        tooltip: 'Verification is currently in progress',
      };
    case 'rejected':
      return {
        label: 'Rejected',
        className: 'bg-red-500/10 text-red-500 border-red-500/20',
        Icon: ShieldX,
        tooltip: 'Verification was rejected',
      };
    case 'unverified':
    default:
      return {
        label: 'Unverified',
        className: 'bg-gray-500/10 text-gray-500 border-gray-500/20',
        Icon: ShieldAlert,
        tooltip: 'This contract has not been verified',
      };
  }
}

export default function VerificationBadge(props: { status: VerificationStatus; level?: VerificationLevel; size?: 'sm' | 'md' }) {
  const { status, level, size = 'sm' } = props;
  const cfg = getBadgeConfig(status, level);

  const iconSize = size === 'md' ? 'w-4 h-4' : 'w-3 h-3';
  const textSize = size === 'md' ? 'text-xs' : 'text-[10px]';
  const padding = size === 'md' ? 'px-2.5 py-1' : 'px-2 py-0.5';

  return (
    <span
      className={`inline-flex items-center gap-1 rounded-full border ${padding} ${textSize} font-semibold uppercase tracking-wide ${cfg.className}`}
      title={cfg.tooltip}
    >
      <cfg.Icon className={iconSize} />
      {cfg.label}
      <Info className="w-3 h-3 ml-1 opacity-70 cursor-help" />
    </span>
  );
}

