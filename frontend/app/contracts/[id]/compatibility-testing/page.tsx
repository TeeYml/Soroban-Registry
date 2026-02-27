'use client';

import { useQuery } from '@tanstack/react-query';
import { useParams } from 'next/navigation';
import Link from 'next/link';
import { api } from '@/lib/api';
import CompatibilityTestingMatrix from '@/components/CompatibilityTestingMatrix';
import { ArrowLeft, FlaskConical } from 'lucide-react';
import Navbar from '@/components/Navbar';

export default function CompatibilityTestingPage() {
    const params = useParams();
    const contractId = params.id as string;

    const { data: contract } = useQuery({
        queryKey: ['contract', contractId],
        queryFn: () => api.getContract(contractId),
    });

    return (
        <div className="min-h-screen bg-background text-foreground">
            <Navbar />
            <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
                {/* Back navigation */}
                <Link
                    href={`/contracts/${contractId}`}
                    className="inline-flex items-center gap-2 text-sm text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white mb-6 transition-colors"
                >
                    <ArrowLeft className="w-4 h-4" />
                    Back to contract
                </Link>

                {/* Header */}
                <div className="mb-8">
                    <div className="flex items-center gap-3 mb-2">
                        <span className="flex items-center justify-center w-9 h-9 rounded-lg bg-purple-100 dark:bg-purple-900/40">
                            <FlaskConical className="w-5 h-5 text-purple-600 dark:text-purple-400" />
                        </span>
                        <h1 className="text-2xl font-bold text-gray-900 dark:text-white">
                            SDK Compatibility Testing
                        </h1>
                    </div>
                    {contract && (
                        <p className="text-gray-500 dark:text-gray-400 ml-12">
                            {contract.name}{' '}
                            <span className="font-mono text-xs bg-gray-100 dark:bg-gray-800 px-1.5 py-0.5 rounded">
                                {contract.contract_id.slice(0, 12)}…
                            </span>
                        </p>
                    )}
                    <p className="text-sm text-gray-400 dark:text-gray-500 mt-1 ml-12">
                        Test contract compatibility across Soroban SDK versions, Wasm runtimes, and Stellar networks.
                    </p>
                </div>

                {/* Content */}
                <div className="bg-white dark:bg-gray-900 rounded-xl border border-gray-200 dark:border-gray-800 p-6">
                    <CompatibilityTestingMatrix contractId={contractId} />
                </div>
            </div>
        </div>
    );
}
