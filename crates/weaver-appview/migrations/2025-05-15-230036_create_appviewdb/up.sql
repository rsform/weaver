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

create table if not exists oauth_requests (
  id serial primary key,
  auth_server_iss text not null,
  state text,
  did text not null,
  pkce_verifier text not null,
  dpop_key jsonb not null
);

create table if not exists oauth_sessions (
  id serial primary key,
  did text not null,
  pds_url text not null,
  session jsonb not null,
  expiry text
);
