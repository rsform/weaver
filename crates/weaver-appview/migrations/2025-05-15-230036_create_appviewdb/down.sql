-- This file should undo anything in `up.sql`
drop index if exists idx_oauth_auth_requests_did;

drop index if exists idx_oauth_auth_requests_expires;

drop index if exists idx_oauth_sessions_did;

drop index if exists idx_oauth_sessions_did_session;

drop table if exists oauth_auth_requests;

drop table if exists oauth_sessions;

drop table if exists profile_pronouns;

drop table if exists profile_links;

drop table if exists profile;

drop table if exists emails;

drop table if exists _jetstream;

drop table if exists follows;

drop table if exists public_keys;

drop table if exists registrations;
