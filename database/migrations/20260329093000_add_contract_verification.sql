-- Add verification_status column and verification_events audit trail
BEGIN;

-- Add 'unverified' value to verification_status enum if it doesn't exist
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_type t
        JOIN pg_enum e ON t.oid = e.enumtypid
        WHERE t.typname = 'verification_status' AND e.enumlabel = 'unverified'
    ) THEN
        ALTER TYPE verification_status ADD VALUE 'unverified';
    END IF;
END$$;

-- Add columns to contracts
ALTER TABLE contracts
    ADD COLUMN verification_status verification_status NOT NULL DEFAULT 'unverified',
    ADD COLUMN verified_by UUID,
    ADD COLUMN verification_notes TEXT;

-- Backfill verification_status from legacy is_verified column
UPDATE contracts SET verification_status = 'verified' WHERE is_verified = true;
UPDATE contracts SET verification_status = 'unverified' WHERE verification_status IS NULL;

-- Create verification_events audit table
CREATE TABLE IF NOT EXISTS verification_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    contract_id UUID NOT NULL REFERENCES contracts(id) ON DELETE CASCADE,
    from_status verification_status NOT NULL,
    to_status verification_status NOT NULL,
    changed_by UUID,
    notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_verification_events_contract_id ON verification_events(contract_id);
CREATE INDEX IF NOT EXISTS idx_verification_events_created_at ON verification_events(created_at);

-- Prevent invalid transition: verified -> unverified
CREATE OR REPLACE FUNCTION prevent_verified_to_unverified()
RETURNS TRIGGER AS $$
BEGIN
    IF OLD.verification_status = 'verified' AND NEW.verification_status = 'unverified' THEN
        RAISE EXCEPTION 'Invalid status transition: verified -> unverified';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_prevent_verified_to_unverified ON contracts;
CREATE TRIGGER trg_prevent_verified_to_unverified
BEFORE UPDATE ON contracts
FOR EACH ROW EXECUTE FUNCTION prevent_verified_to_unverified();

-- Log status changes to verification_events
CREATE OR REPLACE FUNCTION log_verification_event()
RETURNS TRIGGER AS $$
BEGIN
    IF (OLD.verification_status IS DISTINCT FROM NEW.verification_status) THEN
        INSERT INTO verification_events (contract_id, from_status, to_status, changed_by, notes, created_at)
        VALUES (OLD.id, OLD.verification_status, NEW.verification_status, NEW.verified_by, NEW.verification_notes, NOW());
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_log_verification_event ON contracts;
CREATE TRIGGER trg_log_verification_event
AFTER UPDATE ON contracts
FOR EACH ROW EXECUTE FUNCTION log_verification_event();

COMMIT;
