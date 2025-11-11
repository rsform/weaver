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
#[allow(unused)]
const DEFAULT_CURSOR_LIMIT_MAX: u64 = 100;

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
    subject: jacquard::types::uri::Uri<'a>,
    /// Filter links only from this link source
    ///
    /// eg.: `app.bsky.feed.like:subject.uri`
    source: CowStr<'a>,
    #[serde(borrow)]
    cursor: Option<CowStr<'a>>,
    /// Filter links only from these DIDs
    ///
    /// include multiple times to filter by multiple source DIDs
    #[serde(default)]
    did: Vec<Did<'a>>,
    /// Set the max number of links to return per page of results
    #[serde(default = "get_default_cursor_limit")]
    limit: u64,
    // TODO: allow reverse (er, forward) order as well
}
#[derive(Deserialize, Serialize, IntoStatic)]
pub struct GetBacklinksResponse<'a> {
    total: u64,
    #[serde(borrow)]
    records: Vec<RecordId<'a>>,
    cursor: Option<CowStr<'a>>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, IntoStatic)]
pub struct RecordId<'a> {
    #[serde(borrow)]
    pub did: Did<'a>,
    pub collection: Nsid<'a>,
    pub rkey: RecordKey<Rkey<'a>>,
}

impl RecordId<'_> {
    pub fn did(&self) -> Did<'_> {
        self.did.clone()
    }
    pub fn collection(&self) -> Nsid<'_> {
        self.collection.clone()
    }
    pub fn rkey(&self) -> RecordKey<Rkey<'_>> {
        self.rkey.clone()
    }
}
