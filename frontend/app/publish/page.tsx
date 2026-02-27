'use client';

import React from 'react';
import { api } from '@/lib/api';
import useFormValidation, { validators } from '@/lib/formValidation';
import { FormInput, FormSelect, FormTextarea } from '@/components/Form';
import Navbar from '@/components/Navbar';
import { useToast } from '@/hooks/useToast';

type Values = {
  contract_id: string;
  name: string;
  version: string;
  source_url?: string;
  publisher_address: string;
  network: 'mainnet' | 'testnet' | 'futurenet';
  description?: string;
  is_public?: boolean;
};

export default function PublishPage() {
  const { showSuccess, showError } = useToast();

  const { values, errors, handleChange, handleBlur, handleSubmit, setValues } = useFormValidation<Values>({
    initialValues: {
      contract_id: '',
      name: '',
      version: '0.1.0',
      source_url: '',
      publisher_address: '',
      network: 'testnet',
      description: '',
      is_public: true,
    },
    validate: (vals) => {
      const e: Partial<Record<keyof Values, string>> = {};
      if (validators.required(vals.contract_id)) e.contract_id = 'Contract id is required';
      if (validators.required(vals.name)) e.name = 'Name is required';
      const sem = validators.semver(vals.version);
      if (sem) e.version = sem;
      const urlErr = validators.url(vals.source_url);
      if (urlErr) e.source_url = urlErr;
      const sk = validators.stellarPublicKey(vals.publisher_address);
      if (sk) e.publisher_address = sk;
      return e;
    },
    onSubmit: async (vals) => {
      try {
        await api.publishContract({
          contract_id: vals.contract_id,
          name: vals.name,
          description: vals.description,
          network: vals.network,
          category: undefined,
          tags: [],
          source_url: vals.source_url,
          publisher_address: vals.publisher_address,
        });
        showSuccess('Published successfully');
        setValues({
          contract_id: '',
          name: '',
          version: '0.1.0',
          source_url: '',
          publisher_address: '',
          network: 'testnet',
          description: '',
          is_public: true,
        });
      } catch (err: unknown) {
        showError(err instanceof Error ? err.message : 'Failed to publish');
      }
    },
  });

  return (
    <div className="flex flex-col min-h-screen bg-background">
      <Navbar />
      <div className="max-w-3xl mx-auto py-8 px-4 sm:px-6 lg:px-8 w-full flex-grow">
        <h1 className="text-2xl sm:text-3xl font-bold mb-4 text-center sm:text-left">Publish Contract</h1>

        <form
          onSubmit={handleSubmit}
          className="space-y-4 bg-background p-4 sm:p-6 rounded-lg border border-border w-full"
        >
          <FormInput
            label="Contract ID"
            name="contract_id"
            value={values.contract_id}
            onChange={handleChange}
            onBlur={handleBlur}
            error={errors.contract_id}
            placeholder="contract-name"
          />

          <FormInput
            label="Name"
            name="name"
            value={values.name}
            onChange={handleChange}
            onBlur={handleBlur}
            error={errors.name}
          />

          <FormInput
            label="Version"
            name="version"
            value={values.version}
            onChange={handleChange}
            onBlur={handleBlur}
            error={errors.version}
            placeholder="1.2.3"
          />

          <FormInput
            label="Source URL"
            name="source_url"
            value={values.source_url}
            onChange={handleChange}
            onBlur={handleBlur}
            error={errors.source_url}
            placeholder="https://github.com/owner/repo"
          />

          <FormInput
            label="Publisher Stellar Address"
            name="publisher_address"
            value={values.publisher_address}
            onChange={handleChange}
            onBlur={handleBlur}
            error={errors.publisher_address}
            placeholder="G..."
          />

          <FormSelect
            label="Network"
            name="network"
            value={values.network}
            onChange={(e) => handleChange(e as React.ChangeEvent<HTMLInputElement & HTMLSelectElement>)}
            options={[
              { value: 'mainnet', label: 'Mainnet' },
              { value: 'testnet', label: 'Testnet' },
              { value: 'futurenet', label: 'Futurenet' },
            ]}
          />

          <FormTextarea
            label="Description"
            name="description"
            value={values.description}
            onChange={handleChange}
          />

          <div className="flex flex-col sm:flex-row items-center sm:justify-between gap-2">
            <button type="submit" className="w-full sm:w-auto px-4 py-2 rounded-lg bg-primary text-primary-foreground">
              Publish
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
