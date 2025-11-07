create table if not exists registrations (
  id integer not null primary key autoincrement,
  domain text not null unique,
  did text not null,
  secret text not null,
  created timestamp not null default (datetime('now')),
  registered text
);

create table if not exists public_keys (
  id integer not null primary key autoincrement,
  did text not null,
  name text not null,
  key_contents text not null,
  rkey text not null,
  created timestamp not null default (datetime('now')),
  unique (did, name, key_contents)
);

create table if not exists follows (
  user_did text not null,
  subject_did text not null,
  rkey text not null,
  followed_at timestamp not null default (datetime('now')),
  primary key (user_did, subject_did),
    check (user_did <> subject_did)
);

create table if not exists _jetstream (
  id integer not null primary key autoincrement,
  last_time_us integer not null
);

create table if not exists emails (
  id integer not null primary key autoincrement,
  did text not null,
  email text not null,
  verified boolean not null default false,
  verification_code text not null,
  last_sent timestamp not null default (datetime('now')),
  is_primary boolean not null default false,
  created timestamp not null default (datetime('now')),
  unique (did, email)
);

create table if not exists profile (
  -- id
  id integer not null primary key autoincrement,
  did text not null,
  -- data
  avatar text,
  description text not null,
  include_bluesky boolean not null default false,
  include_tangled boolean not null default false,
  location text,
  pinned_post text,
  created_at timestamp default (datetime('now')),
  -- constraints
  unique (did)
);

create table if not exists profile_links (
  -- id
  id integer not null primary key autoincrement,
  did text not null,
  -- data
  link text not null,
  -- constraints
  foreign key (did) references profile (did) on delete cascade
);

create table if not exists profile_pronouns (
  -- id
  id integer not null primary key autoincrement,
  did text not null,
  -- data
  pronoun text not null,
  -- constraints
  foreign key (did) references profile (did) on delete cascade
);

-- OAuth sessions table for jacquard ClientSessionData
create table if not exists oauth_sessions (
  id integer not null primary key autoincrement,
  -- Extracted from ClientSessionData for indexing
  did text not null,
  session_id text not null,
  -- Full ClientSessionData as JSON
  session_data blob not null,
  created_at timestamp not null default (datetime('now')),
  updated_at timestamp not null default (datetime('now')),
  unique (did, session_id)
);

-- OAuth authorization requests table for jacquard AuthRequestData
create table if not exists oauth_auth_requests (
  id integer not null primary key autoincrement,
  -- Extracted from AuthRequestData for indexing
  state text not null unique,
  -- Optional DID if known at auth request time
  account_did text,
  -- Full AuthRequestData as JSON
  auth_req_data blob not null,
  created_at timestamp not null default (datetime('now')),
  expires_at timestamp not null default (datetime('now', '+10 minutes'))
);

-- Index for quick session lookups
create index if not exists idx_oauth_sessions_did_session on oauth_sessions(did, session_id);

-- Index for DID lookups
create index if not exists idx_oauth_sessions_did on oauth_sessions(did);

-- Index for auth request cleanup
create index if not exists idx_oauth_auth_requests_expires on oauth_auth_requests(expires_at);

-- Index for DID lookups in auth requests
create index if not exists idx_oauth_auth_requests_did on oauth_auth_requests(account_did) where account_did is not null;
