-- Void is a state, not a deletion: when an invoice was voided.
alter table invoices add column voided_at timestamptz;
