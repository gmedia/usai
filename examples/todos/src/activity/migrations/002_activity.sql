create table activity (
  id serial primary key,
  todo_id integer not null,
  event text not null,
  at timestamptz not null default now()
);
