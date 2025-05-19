use atrium_api::types::string::{Cid, Did};
use compact_string::CompactString;

pub type MimeType = CompactString;

pub enum BlobLink {
    PDS {
        pds_host: String,
        did: Did,
        cid: Cid,
        mime_type: MimeType,
    },
    BskyCdn(CdnLink),
    WeaverCdn(CdnLink),
}

pub enum CdnLink {
    Avatar(Did, Cid, MimeType),
    Banner(Did, Cid, MimeType),
    Thumbnail(Did, Cid, MimeType),
    PostImage(Did, Cid, MimeType),
    EmbedImage(Did, Cid, MimeType),
}

impl std::fmt::Display for BlobLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}/{}@{}",
            self.url_prefix(),
            self.did().as_str(),
            self.cid().as_ref(),
            self.mime_type().rsplit('/').next().unwrap()
        )
    }
}

impl BlobLink {
    #[inline]
    pub fn url_prefix(&self) -> &str {
        match self {
            BlobLink::PDS { pds_host, .. } => pds_host,
            BlobLink::BskyCdn(CdnLink::Avatar(..)) => "https://cdn.bsky.app/img/avatar/plain/",
            BlobLink::BskyCdn(CdnLink::Banner(..)) => "https://cdn.bsky.app/img/banner/plain/",
            BlobLink::BskyCdn(CdnLink::Thumbnail(..)) => {
                "https://cdn.bsky.app/img/feed_thumbnail/plain/"
            }
            BlobLink::BskyCdn(CdnLink::PostImage(..)) => {
                "https://cdn.bsky.app/img/feed_fullsize/plain/"
            }
            BlobLink::BskyCdn(CdnLink::EmbedImage(..)) => {
                "https://cdn.bsky.app/img/feed_fullsize/plain/"
            }
            BlobLink::WeaverCdn(CdnLink::Avatar(..)) => "https://cdn.weaver.sh/img/avatar/",
            BlobLink::WeaverCdn(CdnLink::Banner(..)) => "https://cdn.weaver.sh/img/full/",
            BlobLink::WeaverCdn(CdnLink::Thumbnail(..)) => "https://cdn.weaver.sh/img/thumbnail/",
            BlobLink::WeaverCdn(CdnLink::PostImage(..)) => "https://cdn.weaver.sh/img/full/",
            BlobLink::WeaverCdn(CdnLink::EmbedImage(..)) => "https://cdn.weaver.sh/img/embed/",
        }
    }

    #[inline]
    pub const fn did(&self) -> &Did {
        match self {
            BlobLink::PDS { did, .. } => did,
            BlobLink::BskyCdn(CdnLink::Avatar(did, ..))
            | BlobLink::BskyCdn(CdnLink::Banner(did, ..))
            | BlobLink::BskyCdn(CdnLink::Thumbnail(did, ..))
            | BlobLink::BskyCdn(CdnLink::PostImage(did, ..))
            | BlobLink::BskyCdn(CdnLink::EmbedImage(did, ..)) => did,
            BlobLink::WeaverCdn(CdnLink::Avatar(did, ..))
            | BlobLink::WeaverCdn(CdnLink::Banner(did, ..))
            | BlobLink::WeaverCdn(CdnLink::Thumbnail(did, ..))
            | BlobLink::WeaverCdn(CdnLink::PostImage(did, ..))
            | BlobLink::WeaverCdn(CdnLink::EmbedImage(did, ..)) => did,
        }
    }

    #[inline]
    pub const fn cid(&self) -> &Cid {
        match self {
            BlobLink::PDS { cid, .. } => cid,
            BlobLink::BskyCdn(CdnLink::Avatar(_, cid, ..))
            | BlobLink::BskyCdn(CdnLink::Banner(_, cid, ..))
            | BlobLink::BskyCdn(CdnLink::Thumbnail(_, cid, ..))
            | BlobLink::BskyCdn(CdnLink::PostImage(_, cid, ..))
            | BlobLink::BskyCdn(CdnLink::EmbedImage(_, cid, ..)) => cid,
            BlobLink::WeaverCdn(CdnLink::Avatar(_, cid, ..))
            | BlobLink::WeaverCdn(CdnLink::Banner(_, cid, ..))
            | BlobLink::WeaverCdn(CdnLink::Thumbnail(_, cid, ..))
            | BlobLink::WeaverCdn(CdnLink::PostImage(_, cid, ..))
            | BlobLink::WeaverCdn(CdnLink::EmbedImage(_, cid, ..)) => cid,
        }
    }

    #[inline]
    pub const fn mime_type(&self) -> &MimeType {
        match self {
            BlobLink::PDS { mime_type, .. } => mime_type,
            BlobLink::BskyCdn(CdnLink::Avatar(_, _, mime_type))
            | BlobLink::BskyCdn(CdnLink::Banner(_, _, mime_type))
            | BlobLink::BskyCdn(CdnLink::Thumbnail(_, _, mime_type))
            | BlobLink::BskyCdn(CdnLink::PostImage(_, _, mime_type))
            | BlobLink::BskyCdn(CdnLink::EmbedImage(_, _, mime_type)) => mime_type,
            BlobLink::WeaverCdn(CdnLink::Avatar(_, _, mime_type))
            | BlobLink::WeaverCdn(CdnLink::Banner(_, _, mime_type))
            | BlobLink::WeaverCdn(CdnLink::Thumbnail(_, _, mime_type))
            | BlobLink::WeaverCdn(CdnLink::PostImage(_, _, mime_type))
            | BlobLink::WeaverCdn(CdnLink::EmbedImage(_, _, mime_type)) => mime_type,
        }
    }
}
