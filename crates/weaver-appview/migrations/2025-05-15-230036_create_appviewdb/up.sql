create table if not exists registrations (
  id serial primary key,
  domain text not null unique,
  did text not null,
  secret text not null,
  created timestamp
  with
    time zone not null default (now () at time zone 'utc'),
    registered text
);

create table if not exists public_keys (
  id serial primary key,
  did text not null,
  name text not null,
  key_contents text not null,
  rkey text not null,
  created timestamp
  with
    time zone not null default (now () at time zone 'utc'),
    unique (did, name, key_contents)
);

create table if not exists follows (
  user_did text not null,
  subject_did text not null,
  rkey text not null,
  followed_at timestamp
  with
    time zone not null default (now () at time zone 'utc'),
    primary key (user_did, subject_did),
    check (user_did <> subject_did)
);

create table if not exists _jetstream (
  id serial primary key,
  last_time_us integer not null
);

create table if not exists emails (
  id serial primary key,
  did text not null,
  email text not null,
  verified integer not null default 0,
  verification_code text not null,
  last_sent timestamp
  with
    time zone not null default (now () at time zone 'utc'),
    is_primary integer not null default 0,
    created timestamp
  with
    time zone not null default (now () at time zone 'utc'),
    unique (did, email)
);

create table if not exists profile (
  -- id
  id serial primary key,
  did text not null,
  -- data
  avatar text,
  description text not null,
  include_bluesky boolean not null default false,
  include_tangled boolean not null default false,
  location text,
  pinned_post jsonb,
  created_at timestamp
  with
    time zone default (now () at time zone 'utc'),
    -- constraints
    unique (did)
);

create table if not exists profile_links (
  -- id
  id serial primary key,
  did text not null,
  -- data
  link text not null,
  -- constraints
  foreign key (did) references profile (did) on delete cascade
);

create table if not exists profile_pronouns (
  -- id
  id serial primary key,
  did text not null,
  -- data
  pronoun text not null,
  -- constraints
  foreign key (did) references profile (did) on delete cascade
);

-- OAuth sessions table for jacquard ClientSessionData
create table if not exists oauth_sessions (
  id serial primary key,
  -- Extracted from ClientSessionData for indexing
  did text not null,
  session_id text not null,
  -- Full ClientSessionData as jsonb
  session_data jsonb not null,
  created_at timestamp with time zone not null default (now() at time zone 'utc'),
  updated_at timestamp with time zone not null default (now() at time zone 'utc'),
  unique (did, session_id)
);

-- OAuth authorization requests table for jacquard AuthRequestData
create table if not exists oauth_auth_requests (
  id serial primary key,
  -- Extracted from AuthRequestData for indexing
  state text not null unique,
  -- Optional DID if known at auth request time
  account_did text,
  -- Full AuthRequestData as jsonb
  auth_req_data jsonb not null,
  created_at timestamp with time zone not null default (now() at time zone 'utc'),
  expires_at timestamp with time zone not null default ((now() at time zone 'utc') + interval '10 minutes')
);

-- Index for quick session lookups
create index if not exists idx_oauth_sessions_did_session on oauth_sessions(did, session_id);

-- Index for DID lookups
create index if not exists idx_oauth_sessions_did on oauth_sessions(did);

-- Index for auth request cleanup
create index if not exists idx_oauth_auth_requests_expires on oauth_auth_requests(expires_at);

-- Index for DID lookups in auth requests
create index if not exists idx_oauth_auth_requests_did on oauth_auth_requests(account_did) where account_did is not null;
