create table if not exists processed (
  id bigint primary key,
  consumer text not null,
  at timestamptz not null default now()
);
