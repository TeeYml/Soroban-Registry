import { Contract } from '@/lib/api';
import { Clock, ExternalLink, Tag } from 'lucide-react';
import Link from 'next/link';
import React from 'react';
import HealthWidget from './HealthWidget';
import { useAnalytics } from '@/hooks/useAnalytics';
import VerificationBadge from '@/components/verification/VerificationBadge';

interface ContractCardProps {
  contract: Contract;
}

export default function ContractCard({ contract }: ContractCardProps) {
  //declare logevent function to track clicks on contract cards
  const { logEvent } = useAnalytics();
  const networkColors = {
    mainnet: 'bg-green-500/10 text-green-600 border-green-500/20',
    testnet: 'bg-blue-500/10 text-blue-600 border-blue-500/20',
    futurenet: 'bg-purple-500/10 text-purple-600 border-purple-500/20',
  };

  return (
    <Link href={`/contracts/${contract.id}`}
          //log event when user clicks on contract card
          onClick={() => 
          logEvent("contract_viewed",{
          contract_id: contract.id,
          contract_name: contract.name,
          network: contract.network
        })
      }>

      <div className="group relative overflow-hidden rounded-2xl border border-border bg-card transition-all card-hover glow-border gradient-border-card">
        {/* Gradient overlay on hover */}
        <div className="absolute inset-0 bg-gradient-to-br from-primary/5 to-secondary/5 opacity-0 transition-opacity group-hover:opacity-100" />

        <div className="relative p-6">
          {/* Header */}
          <div className="flex items-start justify-between mb-3">
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2 mb-1">
                <h3 className="text-lg font-semibold text-foreground group-hover:text-primary transition-colors truncate">
                  {contract.name}
                </h3>
                {contract.is_verified && (
                  <VerificationBadge status="approved" />
                )}
              </div>
              <p className="text-xs text-muted-foreground font-mono">
                {contract.contract_id.slice(0, 8)}...{contract.contract_id.slice(-6)}
              </p>
            </div>

            <span className={`ml-3 px-2.5 py-1 rounded-full text-[10px] font-semibold uppercase tracking-wide border flex-shrink-0 ${networkColors[contract.network]}`}>
              {contract.network}
            </span>
          </div>

          {/* Description */}
          {contract.description && (
            <p className="text-sm text-muted-foreground mb-4 line-clamp-2 leading-relaxed">
              {contract.description}
            </p>
          )}

          {/* Tags */}
          {contract.tags && contract.tags.length > 0 && (
            <div className="flex flex-wrap gap-1.5 mb-4">
              {contract.tags.slice(0, 3).map((tag) => (
                <span
                  key={tag}
                  className="inline-flex items-center gap-1 px-2.5 py-1 rounded-lg bg-primary/10 text-xs text-primary font-medium"
                >
                  <Tag className="w-3 h-3" />
                  {tag}
                </span>
              ))}
              {contract.tags.length > 3 && (
                <span className="px-2 py-1 text-xs text-muted-foreground font-medium">
                  +{contract.tags.length - 3}
                </span>
              )}
            </div>
          )}

          {/* Health Widget */}
          <div onClick={(e: React.MouseEvent) => e.preventDefault()}>
            <HealthWidget contract={contract} />
          </div>

          {/* Footer */}
          <div className="flex items-center justify-between text-xs text-muted-foreground pt-4 mt-4 border-t border-border">
            <div className="flex items-center gap-1.5">
              <Clock className="w-3.5 h-3.5" />
              {new Date(contract.created_at).toLocaleDateString()}
            </div>
            <div className="flex items-center gap-1.5 opacity-0 group-hover:opacity-100 transition-opacity text-primary font-medium">
              <span>View details</span>
              <ExternalLink className="w-3.5 h-3.5" />
            </div>
          </div>
        </div>
      </div>
    </Link>
  );
}
