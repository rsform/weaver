use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ByteRange {
    pub byte_start: usize,
    pub byte_end: usize,
}

impl ByteRange {
    pub fn new(start: i64, end: i64) -> Self {
        Self {
            byte_start: start.max(0) as usize,
            byte_end: end.max(0) as usize,
        }
    }

    pub fn to_range(self) -> Range<usize> {
        self.byte_start..self.byte_end
    }

    pub fn is_empty(&self) -> bool {
        self.byte_start >= self.byte_end
    }

    pub fn start(&self) -> usize {
        self.byte_start
    }

    pub fn end(&self) -> usize {
        self.byte_end
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "$type")]
pub enum FacetFeature<'a> {
    #[serde(rename = "pub.leaflet.richtext.facet#bold")]
    #[serde(alias = "blog.pckt.richtext.facet#bold")]
    Bold,
    #[serde(rename = "pub.leaflet.richtext.facet#italic")]
    #[serde(alias = "blog.pckt.richtext.facet#italic")]
    Italic,
    #[serde(rename = "pub.leaflet.richtext.facet#code")]
    #[serde(alias = "blog.pckt.richtext.facet#code")]
    Code,
    #[serde(rename = "pub.leaflet.richtext.facet#underline")]
    #[serde(alias = "blog.pckt.richtext.facet#underline")]
    Underline,
    #[serde(rename = "pub.leaflet.richtext.facet#strikethrough")]
    #[serde(alias = "blog.pckt.richtext.facet#strikethrough")]
    Strikethrough,
    #[serde(rename = "pub.leaflet.richtext.facet#highlight")]
    #[serde(alias = "blog.pckt.richtext.facet#highlight")]
    Highlight,
    #[serde(rename = "pub.leaflet.richtext.facet#link")]
    #[serde(alias = "blog.pckt.richtext.facet#link")]
    #[serde(alias = "app.bsky.richtext.facet#link")]
    Link {
        #[serde(borrow)]
        uri: &'a str,
    },
    #[serde(rename = "pub.leaflet.richtext.facet#didMention")]
    #[serde(alias = "blog.pckt.richtext.facet#didMention")]
    #[serde(alias = "app.bsky.richtext.facet#mention")]
    DidMention {
        #[serde(borrow)]
        did: &'a str,
    },
    #[serde(rename = "pub.leaflet.richtext.facet#atMention")]
    #[serde(alias = "blog.pckt.richtext.facet#atMention")]
    AtMention {
        #[serde(borrow, rename = "atUri")]
        at_uri: &'a str,
    },
    #[serde(rename = "app.bsky.richtext.facet#tag")]
    Tag {
        #[serde(borrow)]
        tag: &'a str,
    },
    #[serde(rename = "pub.leaflet.richtext.facet#id")]
    #[serde(alias = "blog.pckt.richtext.facet#id")]
    Id {
        #[serde(borrow)]
        id: Option<&'a str>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedFacet<'a> {
    pub index: ByteRange,
    #[serde(borrow)]
    pub features: Vec<FacetFeature<'a>>,
}

impl<'a> From<&'a weaver_api::app_bsky::richtext::facet::Facet<'a>> for NormalizedFacet<'a> {
    fn from(facet: &'a weaver_api::app_bsky::richtext::facet::Facet<'a>) -> Self {
        use weaver_api::app_bsky::richtext::facet::FacetFeaturesItem;

        let index = ByteRange::new(facet.index.byte_start, facet.index.byte_end);

        let features = facet
            .features
            .iter()
            .filter_map(|f| match f {
                FacetFeaturesItem::Mention(m) => Some(FacetFeature::DidMention {
                    did: m.did.as_ref(),
                }),
                FacetFeaturesItem::Link(l) => Some(FacetFeature::Link {
                    uri: l.uri.as_ref(),
                }),
                FacetFeaturesItem::Tag(t) => Some(FacetFeature::Tag {
                    tag: t.tag.as_ref(),
                }),
                FacetFeaturesItem::Unknown(_) => None,
            })
            .collect();

        Self { index, features }
    }
}

impl<'a> From<&'a weaver_api::pub_leaflet::richtext::facet::Facet<'a>> for NormalizedFacet<'a> {
    fn from(facet: &'a weaver_api::pub_leaflet::richtext::facet::Facet<'a>) -> Self {
        use weaver_api::pub_leaflet::richtext::facet::FacetFeaturesItem;

        let index = ByteRange::new(facet.index.byte_start, facet.index.byte_end);

        let features = facet
            .features
            .iter()
            .filter_map(|f| match f {
                FacetFeaturesItem::Bold(_) => Some(FacetFeature::Bold),
                FacetFeaturesItem::Italic(_) => Some(FacetFeature::Italic),
                FacetFeaturesItem::Code(_) => Some(FacetFeature::Code),
                FacetFeaturesItem::Underline(_) => Some(FacetFeature::Underline),
                FacetFeaturesItem::Strikethrough(_) => Some(FacetFeature::Strikethrough),
                FacetFeaturesItem::Highlight(_) => Some(FacetFeature::Highlight),
                FacetFeaturesItem::Link(l) => Some(FacetFeature::Link {
                    uri: l.uri.as_ref(),
                }),
                FacetFeaturesItem::DidMention(m) => Some(FacetFeature::DidMention {
                    did: m.did.as_ref(),
                }),
                FacetFeaturesItem::AtMention(m) => Some(FacetFeature::AtMention {
                    at_uri: m.at_uri.as_ref(),
                }),
                FacetFeaturesItem::Id(i) => Some(FacetFeature::Id {
                    id: i.id.as_ref().map(|s| s.as_ref()),
                }),
                FacetFeaturesItem::Unknown(_) => None,
            })
            .collect();

        Self { index, features }
    }
}

impl<'a> From<&'a weaver_api::blog_pckt::richtext::facet::Facet<'a>> for NormalizedFacet<'a> {
    fn from(facet: &'a weaver_api::blog_pckt::richtext::facet::Facet<'a>) -> Self {
        use weaver_api::blog_pckt::richtext::facet::FacetFeaturesItem;

        let index = ByteRange::new(facet.index.byte_start, facet.index.byte_end);

        let features = facet
            .features
            .iter()
            .filter_map(|f| match f {
                FacetFeaturesItem::Bold(_) => Some(FacetFeature::Bold),
                FacetFeaturesItem::Italic(_) => Some(FacetFeature::Italic),
                FacetFeaturesItem::Code(_) => Some(FacetFeature::Code),
                FacetFeaturesItem::Underline(_) => Some(FacetFeature::Underline),
                FacetFeaturesItem::Strikethrough(_) => Some(FacetFeature::Strikethrough),
                FacetFeaturesItem::Highlight(_) => Some(FacetFeature::Highlight),
                FacetFeaturesItem::Link(l) => Some(FacetFeature::Link {
                    uri: l.uri.as_ref(),
                }),
                FacetFeaturesItem::DidMention(m) => Some(FacetFeature::DidMention {
                    did: m.did.as_ref(),
                }),
                FacetFeaturesItem::AtMention(m) => Some(FacetFeature::AtMention {
                    at_uri: m.at_uri.as_ref(),
                }),
                FacetFeaturesItem::Id(i) => Some(FacetFeature::Id {
                    id: i.id.as_ref().map(|s| s.as_ref()),
                }),
                FacetFeaturesItem::Unknown(_) => None,
            })
            .collect();

        Self { index, features }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_leaflet_facet() {
        let json = r#"{
            "index": {"byteStart": 0, "byteEnd": 5},
            "features": [
                {"$type": "pub.leaflet.richtext.facet#bold"},
                {"$type": "pub.leaflet.richtext.facet#italic"}
            ]
        }"#;

        let facet: NormalizedFacet = serde_json::from_str(json).unwrap();
        assert_eq!(facet.index.byte_start, 0);
        assert_eq!(facet.index.byte_end, 5);
        assert_eq!(facet.features.len(), 2);
        assert!(matches!(facet.features[0], FacetFeature::Bold));
        assert!(matches!(facet.features[1], FacetFeature::Italic));
    }

    #[test]
    fn test_deserialize_bsky_facet() {
        let json = r#"{
            "index": {"byteStart": 0, "byteEnd": 10},
            "features": [
                {"$type": "app.bsky.richtext.facet#link", "uri": "https://example.com"},
                {"$type": "app.bsky.richtext.facet#mention", "did": "did:plc:abc123"}
            ]
        }"#;

        let facet: NormalizedFacet = serde_json::from_str(json).unwrap();
        assert_eq!(facet.index.byte_start, 0);
        assert_eq!(facet.index.byte_end, 10);
        assert_eq!(facet.features.len(), 2);
        assert!(matches!(facet.features[0], FacetFeature::Link { uri: "https://example.com" }));
        assert!(matches!(facet.features[1], FacetFeature::DidMention { did: "did:plc:abc123" }));
    }

    #[test]
    fn test_deserialize_pckt_facet() {
        let json = r#"{
            "index": {"byteStart": 5, "byteEnd": 15},
            "features": [
                {"$type": "blog.pckt.richtext.facet#code"},
                {"$type": "blog.pckt.richtext.facet#strikethrough"}
            ]
        }"#;

        let facet: NormalizedFacet = serde_json::from_str(json).unwrap();
        assert_eq!(facet.index.byte_start, 5);
        assert_eq!(facet.index.byte_end, 15);
        assert_eq!(facet.features.len(), 2);
        assert!(matches!(facet.features[0], FacetFeature::Code));
        assert!(matches!(facet.features[1], FacetFeature::Strikethrough));
    }

    #[test]
    fn test_deserialize_tag_facet() {
        let json = r#"{
            "index": {"byteStart": 0, "byteEnd": 8},
            "features": [
                {"$type": "app.bsky.richtext.facet#tag", "tag": "rust"}
            ]
        }"#;

        let facet: NormalizedFacet = serde_json::from_str(json).unwrap();
        assert!(matches!(facet.features[0], FacetFeature::Tag { tag: "rust" }));
    }
}
