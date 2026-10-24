-- Supabase can install this event-trigger helper in the exposed public schema.
-- Trigger execution needs no client EXECUTE grant. Preserve the trigger itself.
begin;
do $$
begin
    if to_regprocedure('public.rls_auto_enable()') is not null then
        revoke execute on function public.rls_auto_enable() from public, anon, authenticated;
    end if;
end;
$$;
commit;
