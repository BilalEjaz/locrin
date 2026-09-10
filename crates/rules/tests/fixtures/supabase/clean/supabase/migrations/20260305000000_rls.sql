-- The table above is locked down here, three migrations later. The rule reads
-- every migration before it answers, so a later file counts.
ALTER TABLE ONLY public.profiles ENABLE ROW LEVEL SECURITY;

create policy "profiles are readable by their owner" on public.profiles
  for select using (auth.uid() = id);
