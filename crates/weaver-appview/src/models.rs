use chrono::{DateTime, Utc};
use diesel::prelude::*;

#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::schema::profile)]
pub struct Profile {
    pub id: i32,
    pub did: String,
    pub avatar: Option<String>,
    pub description: String,
    pub include_bluesky: bool,
    pub include_tangled: bool,
    pub location: Option<String>,
    pub pinned_post: Option<serde_json::Value>,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::schema::registrations)]
pub struct Registration {
    pub id: i32,
    pub domain: String,
    pub did: String,
    pub secret: String,
    pub created: DateTime<Utc>,
    pub registered: Option<String>,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::schema::public_keys)]
pub struct PublicKey {
    pub id: i32,
    pub did: String,
    pub name: String,
    pub key_contents: String,
    pub rkey: String,
    pub created: DateTime<Utc>,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::schema::follows)]
pub struct Follow {
    pub user_did: String,
    pub subject_did: String,
    pub rkey: String,
    pub followed_at: DateTime<Utc>,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::schema::_jetstream)]
pub struct Jetstream {
    pub id: i32,
    pub last_time_us: i32,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::schema::emails)]
pub struct Email {
    pub id: i32,
    pub did: String,
    pub email: String,
    pub verified: i32,
    pub verification_code: String,
    pub last_sent: DateTime<Utc>,
    pub is_primary: i32,
    pub created: DateTime<Utc>,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::schema::profile_links)]
pub struct ProfileLink {
    pub id: i32,
    pub did: String,
    pub link: String,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::schema::profile_pronouns)]
pub struct ProfilePronoun {
    pub id: i32,
    pub did: String,
    pub pronoun: String,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::schema::oauth_requests)]
pub struct OauthRequest {
    pub id: i32,
    pub auth_server_iss: String,
    pub state: Option<String>,
    pub did: String,
    pub pkce_verifier: String,
    pub dpop_key: serde_json::Value,
}

#[derive(Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::oauth_sessions)]
pub struct OauthSession {
    pub id: i32,
    pub did: String,
    pub pds_url: String,
    pub session: serde_json::Value,
    pub expiry: Option<String>,
}
