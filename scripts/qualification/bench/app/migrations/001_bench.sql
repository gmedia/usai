-- The benchmark's tables. Every comparator runs against this schema; the
-- seed is deterministic so byte-for-byte response checks are possible
-- (docs/measurements/BENCHMARKS.md).
create table users (
  id serial primary key,
  name text not null,
  email text not null unique
);
create table orders (
  id serial primary key,
  user_id integer not null references users (id),
  total_cents integer not null,
  paid boolean not null default false
);
create table payments (
  id serial primary key,
  order_id integer not null references orders (id),
  amount_cents integer not null,
  created_at timestamptz not null default now()
);
create table api_keys (
  key text primary key,
  user_id integer not null references users (id)
);
insert into users (id, name, email)
  select i, 'user-' || i, 'user' || i || '@example.test' from generate_series(1, 10000) as i;
select setval('users_id_seq', 10000);
insert into orders (id, user_id, total_cents, paid)
  select i, ((i - 1) % 10000) + 1, 1000 + (i % 5000), false from generate_series(1, 100000) as i;
select setval('orders_id_seq', 100000);
insert into api_keys (key, user_id)
  select 'key-' || i, i from generate_series(1, 1000) as i;
