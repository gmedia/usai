create table todos (
  id serial primary key,
  title text not null,
  done boolean not null default false,
  created_at timestamptz not null default now(),
  completed_at timestamptz
);
