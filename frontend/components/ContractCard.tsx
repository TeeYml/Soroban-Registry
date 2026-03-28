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
  const router = useRouter();
  const [copied, setCopied] = React.useState(false);
  const networkColors = {
    mainnet: "bg-green-500/10 text-green-600 border-green-500/20",
    testnet: "bg-blue-500/10 text-blue-600 border-blue-500/20",
    futurenet: "bg-purple-500/10 text-purple-600 border-purple-500/20",
  };
  const networkDots = {
    mainnet: "bg-green-500",
    testnet: "bg-blue-500",
    futurenet: "bg-purple-500",
  };

  const creator =
    contract.publisher_id.length > 14
      ? `${contract.publisher_id.slice(0, 8)}...${contract.publisher_id.slice(-4)}`
      : contract.publisher_id;
  const address = `${contract.contract_id.slice(0, 8)}...${contract.contract_id.slice(-6)}`;
  const categoryLabel = contract.category || "uncategorized";
  const deploymentCount =
    typeof (contract as Contract & { deployment_count?: number })
      .deployment_count === "number"
      ? (contract as Contract & { deployment_count?: number }).deployment_count
      : typeof (contract as Contract & { deployments?: number }).deployments ===
          "number"
        ? (contract as Contract & { deployments?: number }).deployments
        : "—";

  const handleViewDetails = (e: React.MouseEvent<HTMLButtonElement>) => {
    e.preventDefault();
    e.stopPropagation();
    router.push(`/contracts/${contract.id}`);
  };

  const handleCopyAddress = async (e: React.MouseEvent<HTMLButtonElement>) => {
    e.preventDefault();
    e.stopPropagation();

    try {
      await navigator.clipboard.writeText(contract.contract_id);
      setCopied(true);
      logEvent("contract_address_copied", {
        contract_id: contract.id,
        contract_name: contract.name,
      });
      setTimeout(() => setCopied(false), 1800);
    } catch {
      logEvent("contract_address_copy_failed", {
        contract_id: contract.id,
        contract_name: contract.name,
      });
    }
  };

  return (
    <Link
      href={`/contracts/${contract.id}`}
      //log event when user clicks on contract card
      onClick={() =>
        logEvent("contract_viewed", {
          contract_id: contract.id,
          contract_name: contract.name,
          network: contract.network,
        })
      }
    >
      <div className="group relative overflow-hidden rounded-2xl border border-border bg-card transition-all card-hover glow-border gradient-border-card h-full">
        {/* Gradient overlay on hover */}
        <div className="absolute inset-0 bg-linear-to-br from-primary/5 to-secondary/5 opacity-0 transition-opacity group-hover:opacity-100" />

        <div className="relative p-6 h-full flex flex-col">
          {/* Header */}
          <div className="flex items-start justify-between mb-3 gap-3">
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

            <span
              className={`ml-3 px-2.5 py-1 rounded-full text-[10px] font-semibold uppercase tracking-wide border shrink-0 inline-flex items-center gap-1.5 ${networkColors[contract.network]}`}
            >
              <span
                className={`w-2 h-2 rounded-full ${networkDots[contract.network]}`}
              />
              {contract.network}
            </span>
          </div>

          <div className="flex items-center justify-between gap-2 mb-4">
            <span className="inline-flex items-center gap-1 px-2.5 py-1 rounded-lg bg-primary/10 text-xs text-primary font-medium max-w-[60%]">
              <Tag className="w-3 h-3 shrink-0" />
              <span className="truncate">{categoryLabel}</span>
            </span>
            <span
              className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-semibold uppercase tracking-wide border shrink-0 ${
                contract.is_verified
                  ? "bg-green-500/10 text-green-500 border-green-500/20"
                  : "bg-yellow-500/10 text-yellow-600 border-yellow-500/20"
              }`}
            >
              <CheckCircle2 className="w-3 h-3" />
              {contract.is_verified ? "verified" : "pending"}
            </span>
          </div>

          {/* Description */}
          {contract.description && (
            <p className="text-sm text-muted-foreground mb-4 line-clamp-2 leading-relaxed">
              {contract.description}
            </p>
          )}

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 mb-4 text-xs text-muted-foreground">
            <div className="flex items-center gap-1.5">
              <Layers3 className="w-3.5 h-3.5" />
              <span>Deployments: {deploymentCount}</span>
            </div>
            <div className="flex items-center gap-1.5 min-w-0">
              <Clock className="w-3.5 h-3.5 shrink-0" />
              <span className="truncate">
                Updated: {new Date(contract.updated_at).toLocaleDateString()}
              </span>
            </div>
          </div>

          <p className="text-xs text-muted-foreground font-mono truncate mb-4">
            {address}
          </p>

          {/* Health Widget */}
          <div
            className="mb-4"
            onClick={(e: React.MouseEvent) => e.preventDefault()}
          >
            <HealthWidget contract={contract} />
          </div>

          {/* Footer */}
          <div className="flex flex-wrap items-center gap-2 pt-4 mt-auto border-t border-border">
            <button
              type="button"
              onClick={handleViewDetails}
              className="inline-flex items-center gap-1 rounded-md border border-border bg-card px-2.5 py-1 text-xs font-medium text-foreground transition-colors hover:bg-accent"
            >
              <span>View Details</span>
              <ExternalLink className="w-3.5 h-3.5" />
            </button>
            <button
              type="button"
              onClick={handleCopyAddress}
              className="inline-flex items-center gap-1 rounded-md border border-border bg-card px-2.5 py-1 text-xs font-medium text-foreground transition-colors hover:bg-accent"
            >
              {copied ? (
                <Check className="w-3.5 h-3.5" />
              ) : (
                <Copy className="w-3.5 h-3.5" />
              )}
              <span>{copied ? "Copied" : "Copy Address"}</span>
            </button>
          </div>
        </div>
      </div>
    </Link>
  );
}
