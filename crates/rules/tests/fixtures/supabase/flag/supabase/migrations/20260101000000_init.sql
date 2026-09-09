-- Two tables, one of them left open.
create table public.profiles (
  id uuid primary key,
  display_name text
);

alter table public.profiles enable row level security;

create table if not exists audit_log (
  id bigserial primary key,
  actor uuid,
  action text not null
);

-- create table public.legacy_notes (id uuid primary key);
