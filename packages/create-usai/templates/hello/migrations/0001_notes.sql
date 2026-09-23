-- The first migration. It does nothing until the application declares a
-- database: uncomment the `postgres()` resource and the `notes` route in
-- src/app.ts, point DATABASE_URL at a server (.env for development), then
--
--   pnpm usai db migrate     # applies this file, records it, takes a lock
--   pnpm usai db status      # what is applied and what is pending
--
-- Migrations are part of the artifact, so the same command works against a
-- built artifact in production: `usai db migrate --artifact /app/.usai/build`.
-- Files are applied in file-name order, once, inside one transaction each.
create table if not exists notes (
  id         bigserial primary key,
  body       text        not null,
  created_at timestamptz not null default now()
);
