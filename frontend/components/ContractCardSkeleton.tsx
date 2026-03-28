import LoadingSkeleton from "./LoadingSkeleton";

export default function ContractCardSkeleton() {
  return (
    <div className="relative overflow-hidden rounded-2xl border border-border bg-card p-6 glow-border h-full">
      <div className="relative h-full flex flex-col">
        {/* Header */}
        <div className="flex items-start justify-between mb-3">
          <div className="flex-1">
            <div className="flex items-center gap-2 mb-1">
              <LoadingSkeleton width="60%" height="1.5rem" />
            </div>
            <LoadingSkeleton width="40%" height="0.875rem" className="mt-2" />
          </div>
          <LoadingSkeleton
            width="5rem"
            height="1.75rem"
            className="rounded-full"
          />
        </div>

        <div className="flex items-center justify-between gap-2 mb-4">
          <LoadingSkeleton
            width="6rem"
            height="1.75rem"
            className="rounded-lg"
          />
          <LoadingSkeleton
            width="4.5rem"
            height="1.25rem"
            className="rounded-full"
          />
        </div>

        {/* Description */}
        <div className="mb-4 space-y-2">
          <LoadingSkeleton width="100%" height="0.875rem" />
          <LoadingSkeleton width="85%" height="0.875rem" />
        </div>

        <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 mb-4">
          <LoadingSkeleton width="100%" height="0.875rem" />
          <LoadingSkeleton width="100%" height="0.875rem" />
        </div>

        <LoadingSkeleton width="55%" height="0.875rem" className="mb-4" />

        {/* Health Widget */}
        <div className="mb-4">
          <LoadingSkeleton width="100%" height="3rem" className="rounded-lg" />
        </div>

        {/* Footer */}
        <div className="flex items-center gap-2 pt-4 mt-auto border-t border-border">
          <LoadingSkeleton
            width="6.5rem"
            height="1.75rem"
            className="rounded-md"
          />
          <LoadingSkeleton
            width="7.5rem"
            height="1.75rem"
            className="rounded-md"
          />
        </div>
      </div>
    </div>
  );
}
