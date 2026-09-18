create type invoice_status as enum ('draft', 'issued', 'paid', 'overdue', 'void');

create table invoices (
  id          uuid primary key default gen_random_uuid(),
  tenant_id   uuid not null references tenants(id) on delete cascade,
  number      integer not null,
  customer    text not null,
  currency    char(3) not null,
  status      invoice_status not null default 'draft',
  total_cents bigint not null default 0,
  due_date    date not null,
  issued_at   timestamptz,
  paid_at     timestamptz,
  created_at  timestamptz not null default now(),
  unique (tenant_id, number)
);
create index invoices_page on invoices (tenant_id, created_at desc, id desc);

create table invoice_items (
  id          bigserial primary key,
  invoice_id  uuid not null references invoices(id) on delete cascade,
  description text not null,
  quantity    integer not null check (quantity > 0),
  unit_cents  bigint not null check (unit_cents >= 0)
);

create table webhook_deliveries (
  id          bigserial primary key,
  tenant_id   uuid not null references tenants(id) on delete cascade,
  event       text not null,
  invoice_id  uuid not null,
  attempt     integer not null,
  status      integer,
  error       text,
  delivered_at timestamptz not null default now()
);
