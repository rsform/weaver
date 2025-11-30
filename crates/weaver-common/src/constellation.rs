use jacquard::{
    CowStr, IntoStatic, XrpcRequest,
    types::{
        did::Did,
        nsid::Nsid,
        string::{RecordKey, Rkey},
    },
};
use serde::{Deserialize, Serialize};

const DEFAULT_CURSOR_LIMIT: u64 = 16;

fn get_default_cursor_limit() -> u64 {
    DEFAULT_CURSOR_LIMIT
}

#[derive(Clone, Deserialize, Serialize, XrpcRequest, IntoStatic)]
#[xrpc(
    nsid = "blue.microcosm.links.getBacklinks",
    method = Query,
    output = GetBacklinksResponse,
)]
pub struct GetBacklinksQuery<'a> {
    /// The link target
    ///
    /// can be an AT-URI, plain DID, or regular URI
    pub subject: jacquard::types::uri::Uri<'a>,
    /// Filter links only from this link source
    ///
    /// eg.: `app.bsky.feed.like:subject.uri`
    pub source: CowStr<'a>,
    #[serde(borrow)]
    pub cursor: Option<CowStr<'a>>,
    /// Filter links only from these DIDs
    ///
    /// include multiple times to filter by multiple source DIDs
    #[serde(default)]
    pub did: Vec<Did<'a>>,
    /// Set the max number of links to return per page of results
    #[serde(default = "get_default_cursor_limit")]
    pub limit: u64,
    // TODO: allow reverse (er, forward) order as well
}
#[derive(Deserialize, Serialize, IntoStatic)]
pub struct GetBacklinksResponse<'a> {
    pub total: u64,
    #[serde(borrow)]
    pub records: Vec<RecordId<'a>>,
    pub cursor: Option<CowStr<'a>>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, IntoStatic)]
pub struct RecordId<'a> {
    #[serde(borrow)]
    pub did: Did<'a>,
    pub collection: Nsid<'a>,
    pub rkey: RecordKey<Rkey<'a>>,
}

