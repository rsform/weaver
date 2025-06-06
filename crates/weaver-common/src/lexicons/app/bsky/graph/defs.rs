// @generated - This file is generated by esquema-codegen (forked from atrium-codegen). DO NOT EDIT.
//!Definitions for the `app.bsky.graph.defs` namespace.
///A list of actors used for curation purposes such as list feeds or interaction gating.
pub const CURATELIST: &str = "app.bsky.graph.defs#curatelist";
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListItemViewData {
    pub subject: crate::app::bsky::actor::defs::ProfileView,
    pub uri: String,
}
pub type ListItemView = atrium_api::types::Object<ListItemViewData>;
pub type ListPurpose = String;
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListViewData {
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub avatar: core::option::Option<String>,
    pub cid: atrium_api::types::string::Cid,
    pub creator: crate::app::bsky::actor::defs::ProfileView,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub description: core::option::Option<String>,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub description_facets: core::option::Option<
        Vec<crate::app::bsky::richtext::facet::Main>,
    >,
    pub indexed_at: atrium_api::types::string::Datetime,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub labels: core::option::Option<Vec<crate::com::atproto::label::defs::Label>>,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub list_item_count: core::option::Option<usize>,
    pub name: String,
    pub purpose: ListPurpose,
    pub uri: String,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub viewer: core::option::Option<ListViewerState>,
}
pub type ListView = atrium_api::types::Object<ListViewData>;
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListViewBasicData {
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub avatar: core::option::Option<String>,
    pub cid: atrium_api::types::string::Cid,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub indexed_at: core::option::Option<atrium_api::types::string::Datetime>,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub labels: core::option::Option<Vec<crate::com::atproto::label::defs::Label>>,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub list_item_count: core::option::Option<usize>,
    pub name: String,
    pub purpose: ListPurpose,
    pub uri: String,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub viewer: core::option::Option<ListViewerState>,
}
pub type ListViewBasic = atrium_api::types::Object<ListViewBasicData>;
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListViewerStateData {
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub blocked: core::option::Option<String>,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub muted: core::option::Option<bool>,
}
pub type ListViewerState = atrium_api::types::Object<ListViewerStateData>;
///A list of actors to apply an aggregate moderation action (mute/block) on.
pub const MODLIST: &str = "app.bsky.graph.defs#modlist";
///indicates that a handle or DID could not be resolved
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NotFoundActorData {
    pub actor: atrium_api::types::string::AtIdentifier,
    pub not_found: bool,
}
pub type NotFoundActor = atrium_api::types::Object<NotFoundActorData>;
///A list of actors used for only for reference purposes such as within a starter pack.
pub const REFERENCELIST: &str = "app.bsky.graph.defs#referencelist";
///lists the bi-directional graph relationships between one actor (not indicated in the object), and the target actors (the DID included in the object)
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RelationshipData {
    pub did: atrium_api::types::string::Did,
    ///if the actor is followed by this DID, contains the AT-URI of the follow record
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub followed_by: core::option::Option<String>,
    ///if the actor follows this DID, this is the AT-URI of the follow record
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub following: core::option::Option<String>,
}
pub type Relationship = atrium_api::types::Object<RelationshipData>;
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StarterPackViewData {
    pub cid: atrium_api::types::string::Cid,
    pub creator: crate::app::bsky::actor::defs::ProfileViewBasic,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub feeds: core::option::Option<Vec<crate::app::bsky::feed::defs::GeneratorView>>,
    pub indexed_at: atrium_api::types::string::Datetime,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub joined_all_time_count: core::option::Option<usize>,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub joined_week_count: core::option::Option<usize>,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub labels: core::option::Option<Vec<crate::com::atproto::label::defs::Label>>,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub list: core::option::Option<ListViewBasic>,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub list_items_sample: core::option::Option<Vec<ListItemView>>,
    pub record: atrium_api::types::Unknown,
    pub uri: String,
}
pub type StarterPackView = atrium_api::types::Object<StarterPackViewData>;
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StarterPackViewBasicData {
    pub cid: atrium_api::types::string::Cid,
    pub creator: crate::app::bsky::actor::defs::ProfileViewBasic,
    pub indexed_at: atrium_api::types::string::Datetime,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub joined_all_time_count: core::option::Option<usize>,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub joined_week_count: core::option::Option<usize>,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub labels: core::option::Option<Vec<crate::com::atproto::label::defs::Label>>,
    #[serde(skip_serializing_if = "core::option::Option::is_none")]
    pub list_item_count: core::option::Option<usize>,
    pub record: atrium_api::types::Unknown,
    pub uri: String,
}
pub type StarterPackViewBasic = atrium_api::types::Object<StarterPackViewBasicData>;
