create table public.profiles (
  id uuid primary key,
  display_name text
);

-- A table in Supabase's own schema. Row level security there is Supabase's
-- business, not this repository's, so the rule says nothing about it.
create table auth.audit_entries (
  id uuid primary key,
  payload jsonb
);
