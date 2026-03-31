import type { ContractSearchParams } from '@/lib/api';
import { Check, ChevronDown, RotateCcw } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';

type NetworkFilter = NonNullable<ContractSearchParams['network']>;
type CategoryOption = {
  value: string;
  label: string;
  count: number;
};

interface FilterPanelProps {
  categories: CategoryOption[];
  selectedCategories: string[];
  onToggleCategory: (value: string) => void;
  onClearCategories: () => void;
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

function CategoryMultiSelect({
  categories,
  selectedCategories,
  onToggleCategory,
  onClearCategories,
}: Pick<
  FilterPanelProps,
  'categories' | 'selectedCategories' | 'onToggleCategory' | 'onClearCategories'
>) {
  const [isOpen, setIsOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!isOpen) return;

    const handlePointerDown = (event: MouseEvent) => {
      if (!containerRef.current?.contains(event.target as Node)) {
        setIsOpen(false);
      }
    };

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setIsOpen(false);
      }
    };

    document.addEventListener('mousedown', handlePointerDown);
    document.addEventListener('keydown', handleEscape);
    return () => {
      document.removeEventListener('mousedown', handlePointerDown);
      document.removeEventListener('keydown', handleEscape);
    };
  }, [isOpen]);

  const selectedLabels = useMemo(
    () =>
      categories
        .filter((option) => selectedCategories.includes(option.value))
        .map((option) => option.label),
    [categories, selectedCategories],
  );

  const triggerLabel =
    selectedLabels.length === 0
      ? 'Choose categories'
      : selectedLabels.length <= 2
        ? selectedLabels.join(', ')
        : `${selectedLabels.length} categories selected`;

  return (
    <div className="space-y-3" ref={containerRef}>
      <div className="flex items-center justify-between gap-3">
        <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
          Category
        </p>
        <button
          type="button"
          onClick={onClearCategories}
          disabled={selectedCategories.length === 0}
          className="inline-flex items-center gap-1 text-xs text-muted-foreground transition-colors hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50"
        >
          <RotateCcw className="w-3 h-3" />
          Reset
        </button>
      </div>

      <div className="relative">
        <button
          type="button"
          onClick={() => setIsOpen((current) => !current)}
          aria-expanded={isOpen}
          className="flex w-full items-center justify-between rounded-xl border border-border bg-background px-3 py-2.5 text-left text-sm text-foreground transition-all hover:border-primary/40 focus:outline-none focus:ring-2 focus:ring-primary/20"
        >
          <span className={selectedCategories.length === 0 ? 'text-muted-foreground' : ''}>
            {triggerLabel}
          </span>
          <ChevronDown
            className={`h-4 w-4 text-muted-foreground transition-transform ${
              isOpen ? 'rotate-180' : ''
            }`}
          />
        </button>

        {selectedCategories.length > 0 && (
          <div className="mt-2 flex flex-wrap gap-2">
            {categories
              .filter((option) => selectedCategories.includes(option.value))
              .map((option) => (
                <button
                  key={option.value}
                  type="button"
                  onClick={() => onToggleCategory(option.value)}
                  className="inline-flex items-center gap-1 rounded-full border border-primary/20 bg-primary/10 px-2.5 py-1 text-xs font-medium text-primary"
                >
                  {option.label}
                  <Check className="h-3 w-3" />
                </button>
              ))}
          </div>
        )}

        {isOpen && (
          <div className="absolute z-20 mt-2 w-full overflow-hidden rounded-2xl border border-border bg-popover shadow-xl">
            <div className="max-h-72 overflow-y-auto p-2">
              {categories.map((option) => {
                const isSelected = selectedCategories.includes(option.value);
                return (
                  <button
                    key={option.value}
                    type="button"
                    onClick={() => onToggleCategory(option.value)}
                    className={`flex w-full items-center justify-between rounded-xl px-3 py-2 text-sm transition-all ${
                      isSelected
                        ? 'bg-primary/10 text-primary'
                        : 'text-foreground hover:bg-accent'
                    }`}
                  >
                    <span className="flex items-center gap-2">
                      <span
                        className={`flex h-4 w-4 items-center justify-center rounded border ${
                          isSelected
                            ? 'border-primary bg-primary text-primary-foreground'
                            : 'border-border'
                        }`}
                      >
                        {isSelected ? <Check className="h-3 w-3" /> : null}
                      </span>
                      <span>{option.label}</span>
                    </span>
                    <span className="rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">
                      {option.count}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>
        )}
      </div>
    </div>
  );
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
      <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wider mb-3">
        {title}
      </p>
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
              <div
                className={`w-4 h-4 rounded border flex items-center justify-center flex-shrink-0 transition-colors ${
                  isSelected ? 'bg-primary border-primary' : 'border-border'
                }`}
              >
                {isSelected && (
                  <svg
                    className="w-3 h-3 text-primary-foreground"
                    fill="none"
                    viewBox="0 0 24 24"
                    stroke="currentColor"
                    strokeWidth={3}
                  >
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
  onClearCategories,
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
}: FilterPanelProps) {
  return (
    <div className="space-y-5">
      <CategoryMultiSelect
        categories={categories}
        selectedCategories={selectedCategories}
        onToggleCategory={onToggleCategory}
        onClearCategories={onClearCategories}
      />

      <CheckboxGroup
        title="Language"
        options={languages}
        selected={selectedLanguages}
        onToggle={onToggleLanguage}
      />

      <div>
        <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wider mb-3">
          Network
        </p>
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
                <div
                  className={`w-4 h-4 rounded border flex items-center justify-center flex-shrink-0 transition-colors ${
                    isSelected ? 'bg-primary border-primary' : 'border-border'
                  }`}
                >
                  {isSelected && (
                    <svg
                      className="w-3 h-3 text-primary-foreground"
                      fill="none"
                      viewBox="0 0 24 24"
                      stroke="currentColor"
                      strokeWidth={3}
                    >
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
        <div
          className={`w-4 h-4 rounded border flex items-center justify-center flex-shrink-0 transition-colors ${
            verifiedOnly ? 'bg-green-500 border-green-500' : 'border-border'
          }`}
        >
          {verifiedOnly && (
            <svg
              className="w-3 h-3 text-white"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              strokeWidth={3}
            >
              <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
            </svg>
          )}
        </div>
        Verified only
      </button>
    </div>
  );
}
