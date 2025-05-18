// @generated automatically by Diesel CLI.

diesel::table! {
    _jetstream (id) {
        id -> Int4,
        last_time_us -> Int4,
    }
}

diesel::table! {
    emails (id) {
        id -> Int4,
        did -> Text,
        email -> Text,
        verified -> Int4,
        verification_code -> Text,
        last_sent -> Timestamptz,
        is_primary -> Int4,
        created -> Timestamptz,
    }
}

diesel::table! {
    follows (user_did, subject_did) {
        user_did -> Text,
        subject_did -> Text,
        rkey -> Text,
        followed_at -> Timestamptz,
    }
}

diesel::table! {
    oauth_requests (id) {
        id -> Int4,
        auth_server_iss -> Text,
        state -> Nullable<Text>,
        did -> Text,
        pkce_verifier -> Text,
        dpop_key -> Jsonb,
    }
}

diesel::table! {
    oauth_sessions (id) {
        id -> Int4,
        did -> Text,
        pds_url -> Text,
        session -> Jsonb,
        expiry -> Nullable<Text>,
    }
}

diesel::table! {
    profile (id) {
        id -> Int4,
        did -> Text,
        avatar -> Nullable<Text>,
        description -> Text,
        include_bluesky -> Bool,
        include_tangled -> Bool,
        location -> Nullable<Text>,
        pinned_post -> Nullable<Jsonb>,
        created_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    profile_links (id) {
        id -> Int4,
        did -> Text,
        link -> Text,
    }
}

diesel::table! {
    profile_pronouns (id) {
        id -> Int4,
        did -> Text,
        pronoun -> Text,
    }
}

diesel::table! {
    public_keys (id) {
        id -> Int4,
        did -> Text,
        name -> Text,
        key_contents -> Text,
        rkey -> Text,
        created -> Timestamptz,
    }
}

diesel::table! {
    registrations (id) {
        id -> Int4,
        domain -> Text,
        did -> Text,
        secret -> Text,
        created -> Timestamptz,
        registered -> Nullable<Text>,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    _jetstream,
    emails,
    follows,
    oauth_requests,
    oauth_sessions,
    profile,
    profile_links,
    profile_pronouns,
    public_keys,
    registrations,
);
