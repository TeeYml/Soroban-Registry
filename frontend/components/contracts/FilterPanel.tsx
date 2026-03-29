import type { ContractSearchParams } from '@/lib/api';

type NetworkFilter = NonNullable<ContractSearchParams['network']>;

interface FilterPanelProps {
  categories: string[];
  selectedCategories: string[];
  onToggleCategory: (value: string) => void;
  languages: string[];
  selectedLanguages: string[];
  onToggleLanguage: (value: string) => void;
  networks: NetworkFilter[];
  selectedNetworks: NetworkFilter[];
  onToggleNetwork: (value: NetworkFilter) => void;
  author: string;
  onAuthorChange: (value: string) => void;
  verifiedOnly: boolean;
  onVerifiedChange: (value: boolean) => void;
  dateFrom?: string;
  dateTo?: string;
  onDateRangeChange?: (from: string, to: string) => void;
  activeCounts?: Record<string, number>;
  onClearAll?: () => void;
}

function CheckboxGroup({
  title,
  options,
  selected,
  onToggle,
}: {
  title: string;
  options: string[];
  selected: string[];
  onToggle: (value: string) => void;
}) {
  return (
    <div>
      <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wider mb-3">{title}</p>
      <div className="space-y-1.5">
        {options.map((option) => {
          const isSelected = selected.includes(option);
          return (
            <button
              key={option}
              type="button"
              onClick={() => onToggle(option)}
              className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm transition-all ${
                isSelected
                  ? 'bg-primary/10 text-primary font-medium'
                  : 'text-muted-foreground hover:text-foreground hover:bg-accent'
              }`}
            >
              <div className={`w-4 h-4 rounded border flex items-center justify-center flex-shrink-0 transition-colors ${
                isSelected ? 'bg-primary border-primary' : 'border-border'
              }`}>
                {isSelected && (
                  <svg className="w-3 h-3 text-primary-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
                  </svg>
                )}
              </div>
              {option}
            </button>
          );
        })}
      </div>
    </div>
  );
}

export function FilterPanel({
  categories,
  selectedCategories,
  onToggleCategory,
  languages,
  selectedLanguages,
  onToggleLanguage,
  networks,
  selectedNetworks,
  onToggleNetwork,
  author,
  onAuthorChange,
  verifiedOnly,
  onVerifiedChange,
  dateFrom,
  dateTo,
  onDateRangeChange,
  activeCounts = {},
  onClearAll,
}: FilterPanelProps) {
  return (
    <div className="space-y-5">
      <CheckboxGroup
        title="Category"
        options={categories}
        selected={selectedCategories}
        onToggle={onToggleCategory}
      />

      <CheckboxGroup
        title="Language"
        options={languages}
        selected={selectedLanguages}
        onToggle={onToggleLanguage}
      />

      <div>
        <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wider mb-3">Network</p>
        <div className="space-y-1.5">
          {networks.map((network) => {
            const isSelected = selectedNetworks.includes(network);
            return (
              <button
                key={network}
                type="button"
                onClick={() => onToggleNetwork(network)}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm capitalize transition-all ${
                  isSelected
                    ? 'bg-primary/10 text-primary font-medium'
                    : 'text-muted-foreground hover:text-foreground hover:bg-accent'
                }`}
              >
                <div className={`w-4 h-4 rounded border flex items-center justify-center flex-shrink-0 transition-colors ${
                  isSelected ? 'bg-primary border-primary' : 'border-border'
                }`}>
                  {isSelected && (
                    <svg className="w-3 h-3 text-primary-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
                    </svg>
                  )}
                </div>
                {network}
              </button>
            );
          })}
        </div>
      </div>

      <div>
        <label className="block text-xs font-semibold text-muted-foreground uppercase tracking-wider mb-3">
          Author
        </label>
        <input
          type="text"
          value={author}
          onChange={(e) => onAuthorChange(e.target.value)}
          placeholder="Publisher or address"
          className="w-full px-3 py-2 rounded-lg border border-border bg-background text-sm text-foreground placeholder-muted-foreground focus:outline-none focus:ring-2 focus:ring-primary/20 transition-all"
        />
      </div>

      <button
        type="button"
        onClick={() => onVerifiedChange(!verifiedOnly)}
        className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm transition-all ${
          verifiedOnly
            ? 'bg-green-500/10 text-green-600 font-medium'
            : 'text-muted-foreground hover:text-foreground hover:bg-accent'
        }`}
      >
        <div className={`w-4 h-4 rounded border flex items-center justify-center flex-shrink-0 transition-colors ${
          verifiedOnly ? 'bg-green-500 border-green-500' : 'border-border'
        }`}>
          {verifiedOnly && (
            <svg className="w-3 h-3 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
            </svg>
          )}
        </div>
        Verified only
      </button>
    </div>
  );
}
