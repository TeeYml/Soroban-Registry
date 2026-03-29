'use client';

import React from 'react';
import { QueryNode, FieldOperator } from '@/lib/api';
import { Info } from 'lucide-react';

interface QuerySummaryProps {
  query: QueryNode;
  className?: string;
}

const FIELD_LABELS: Record<string, string> = {
  name: 'Name',
  description: 'Description',
  category: 'Category',
  network: 'Network',
  verified: 'Verification',
  publisher: 'Publisher',
};

const OP_LABELS: Record<FieldOperator, string> = {
  eq: 'is',
  ne: 'is not',
  gt: 'is greater than',
  lt: 'is less than',
  in: 'is one of',
  contains: 'contains',
  starts_with: 'starts with',
};

export default function QuerySummary({ query, className = '' }: QuerySummaryProps) {
  const renderNode = (node: QueryNode): string => {
    if ('children' in node) {
      const conditions = (node as any).children || (node as any).conditions || [];
      if (conditions.length === 0) return '';
      
      const parts = conditions
        .map((c: QueryNode) => renderNode(c))
        .filter(Boolean);
        
      if (parts.length === 1) return parts[0];
      
      const op = (node as any).operator || 'AND';
      const joined = parts.join(` ${op} `);
      return `(${joined})`;
    }

    const field = FIELD_LABELS[(node as any).field] || (node as any).field;
    const op = OP_LABELS[(node as any).operator as FieldOperator] || (node as any).operator;
    let val = (node as any).value;
    
    if ((node as any).field === 'verified') {
      val = (node as any).value ? 'Verified' : 'Unverified';
    } else if (Array.isArray((node as any).value)) {
      val = `[${(node as any).value.join(', ')}]`;
    } else if (typeof (node as any).value === 'string') {
      val = `'${(node as any).value}'`;
    }

    return `${field} ${op} ${val}`;
  };

  const summary = renderNode(query);

  if (!summary) return null;

  return (
    <div className={`flex items-start gap-3 p-4 bg-primary/5 border border-primary/20 rounded-xl ${className}`}>
      <div className="p-2 bg-primary/10 rounded-lg shrink-0">
        <Info className="w-4 h-4 text-primary" />
      </div>
      <div>
        <h4 className="text-xs font-bold text-primary uppercase tracking-wider mb-1">Query Summary</h4>
        <p className="text-sm text-foreground leading-relaxed font-medium">
          Showing contracts where <span className="text-primary">{summary}</span>
        </p>
      </div>
    </div>
  );
}
