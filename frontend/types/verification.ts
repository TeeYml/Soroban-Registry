export type VerificationStatus = 'draft' | 'submitted' | 'under_review' | 'approved' | 'rejected';

export type SorobanNetwork = 'mainnet' | 'testnet' | 'futurenet';

export type RiskLevel = 'low' | 'medium' | 'high' | 'critical';

export type AuditStatus = 'not_audited' | 'in_progress' | 'audited';

export type VerificationStepKey = 'contractInfo' | 'description' | 'securityClaims' | 'documents' | 'review';

export type VerificationDocument = {
  id: string;
  name: string;
  sizeBytes: number;
  mimeType: string;
  uploadedAt: string;
};

export type VerificationDraft = {
  contractName: string;
  contractAddress: string;
  network: SorobanNetwork;

  purpose: string;
  useCase: string;

  auditStatus: AuditStatus;
  knownVulnerabilities: string;
  riskLevel: RiskLevel;
};

export type VerificationSubmission = VerificationDraft & {
  documents: VerificationDocument[];
};

export type StatusEvent = {
  status: VerificationStatus;
  at: string;
  message?: string;
};

export type VerificationRequest = {
  id: string;
  createdAt: string;
  updatedAt: string;
  status: VerificationStatus;
  submission: VerificationSubmission;
  statusHistory: StatusEvent[];
};

export type VerificationStatusResponse = {
  request: VerificationRequest;
};

export type VerificationStatusChangeEvent = {
  id: string;
  status: VerificationStatus;
};

