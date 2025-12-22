use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

impl ByteRange {
    pub fn new(start: i64, end: i64) -> Self {
        Self {
            start: start.max(0) as usize,
            end: end.max(0) as usize,
        }
    }

    pub fn to_range(self) -> Range<usize> {
        self.start..self.end
    }

    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FacetFeature<'a> {
    Bold,
    Italic,
    Code,
    Underline,
    Strikethrough,
    Highlight,
    Link { uri: &'a str },
    DidMention { did: &'a str },
    AtMention { at_uri: &'a str },
    Tag { tag: &'a str },
    Id { id: Option<&'a str> },
}

#[derive(Debug, Clone)]
pub struct NormalizedFacet<'a> {
    pub range: ByteRange,
    pub features: Vec<FacetFeature<'a>>,
}

impl<'a> From<&'a weaver_api::app_bsky::richtext::facet::Facet<'a>> for NormalizedFacet<'a> {
    fn from(facet: &'a weaver_api::app_bsky::richtext::facet::Facet<'a>) -> Self {
        use weaver_api::app_bsky::richtext::facet::FacetFeaturesItem;

        let range = ByteRange::new(facet.index.byte_start, facet.index.byte_end);

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

        Self { range, features }
    }
}

impl<'a> From<&'a weaver_api::pub_leaflet::richtext::facet::Facet<'a>> for NormalizedFacet<'a> {
    fn from(facet: &'a weaver_api::pub_leaflet::richtext::facet::Facet<'a>) -> Self {
        use weaver_api::pub_leaflet::richtext::facet::FacetFeaturesItem;

        let range = ByteRange::new(facet.index.byte_start, facet.index.byte_end);

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

        Self { range, features }
    }
}

impl<'a> From<&'a weaver_api::blog_pckt::richtext::facet::Facet<'a>> for NormalizedFacet<'a> {
    fn from(facet: &'a weaver_api::blog_pckt::richtext::facet::Facet<'a>) -> Self {
        use weaver_api::blog_pckt::richtext::facet::FacetFeaturesItem;

        let range = ByteRange::new(facet.index.byte_start, facet.index.byte_end);

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

        Self { range, features }
    }
}
