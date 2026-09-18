create table tenants (
  id            uuid primary key default gen_random_uuid(),
  slug          text not null unique,
  name          text not null,
  webhook_url   text,
  webhook_secret text,
  invoice_seq   integer not null default 0,
  created_at    timestamptz not null default now()
);

create table users (
  id            uuid primary key default gen_random_uuid(),
  tenant_id     uuid not null references tenants(id) on delete cascade,
  email         text not null,
  password_hash text not null,
  created_at    timestamptz not null default now(),
  unique (tenant_id, email)
);

create table sessions (
  token_hash    text primary key,
  user_id       uuid not null references users(id) on delete cascade,
  expires_at    timestamptz not null,
  created_at    timestamptz not null default now()
);
create index sessions_user on sessions (user_id);
