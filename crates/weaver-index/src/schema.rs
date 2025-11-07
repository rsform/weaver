// @generated automatically by Diesel CLI.

diesel::table! {
    _jetstream (id) {
        id -> Integer,
        last_time_us -> Integer,
    }
}

diesel::table! {
    emails (id) {
        id -> Integer,
        did -> Text,
        email -> Text,
        verified -> Bool,
        verification_code -> Text,
        last_sent -> Timestamp,
        is_primary -> Bool,
        created -> Timestamp,
    }
}

diesel::table! {
    follows (user_did, subject_did) {
        user_did -> Text,
        subject_did -> Text,
        rkey -> Text,
        followed_at -> Timestamp,
    }
}

diesel::table! {
    oauth_auth_requests (id) {
        id -> Integer,
        state -> Text,
        account_did -> Nullable<Text>,
        auth_req_data -> Jsonb,
        created_at -> Timestamp,
        expires_at -> Timestamp,
    }
}

diesel::table! {
    oauth_sessions (id) {
        id -> Integer,
        did -> Text,
        session_id -> Text,
        session_data -> Jsonb,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    profile (id) {
        id -> Integer,
        did -> Text,
        avatar -> Nullable<Text>,
        description -> Text,
        include_bluesky -> Bool,
        include_tangled -> Bool,
        location -> Nullable<Text>,
        pinned_post -> Nullable<Text>,
        created_at -> Nullable<Timestamp>,
    }
}

diesel::table! {
    profile_links (id) {
        id -> Integer,
        did -> Text,
        link -> Text,
    }
}

diesel::table! {
    profile_pronouns (id) {
        id -> Integer,
        did -> Text,
        pronoun -> Text,
    }
}

diesel::table! {
    public_keys (id) {
        id -> Integer,
        did -> Text,
        name -> Text,
        key_contents -> Text,
        rkey -> Text,
        created -> Timestamp,
    }
}

diesel::table! {
    registrations (id) {
        id -> Integer,
        domain -> Text,
        did -> Text,
        secret -> Text,
        created -> Timestamp,
        registered -> Nullable<Text>,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    _jetstream,
    emails,
    follows,
    oauth_auth_requests,
    oauth_sessions,
    profile,
    profile_links,
    profile_pronouns,
    public_keys,
    registrations,
);
