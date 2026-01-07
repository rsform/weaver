use super::types::{FacetFeature, NormalizedFacet};
use super::FacetOutput;
use std::cmp::Ordering;

#[derive(Debug, Clone)]
struct FacetEvent<'a> {
    pos: usize,
    is_start: bool,
    feature: FacetFeature<'a>,
    facet_idx: usize,
}

impl<'a> FacetEvent<'a> {
    fn start(pos: usize, feature: FacetFeature<'a>, facet_idx: usize) -> Self {
        Self {
            pos,
            is_start: true,
            feature,
            facet_idx,
        }
    }

    fn end(pos: usize, feature: FacetFeature<'a>, facet_idx: usize) -> Self {
        Self {
            pos,
            is_start: false,
            feature,
            facet_idx,
        }
    }
}

impl<'a> PartialEq for FacetEvent<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.pos == other.pos && self.is_start == other.is_start
    }
}

impl<'a> Eq for FacetEvent<'a> {}

impl<'a> PartialOrd for FacetEvent<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a> Ord for FacetEvent<'a> {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.pos.cmp(&other.pos) {
            Ordering::Equal => {
                // At same position: ends before starts for proper nesting
                match (self.is_start, other.is_start) {
                    (false, true) => Ordering::Less,
                    (true, false) => Ordering::Greater,
                    _ => Ordering::Equal,
                }
            }
            ord => ord,
        }
    }
}

pub fn process_faceted_text<'a, O: FacetOutput>(
    text: &'a str,
    facets: &[NormalizedFacet<'a>],
    output: &mut O,
) -> Result<(), O::Error> {
    let mut events: Vec<FacetEvent<'a>> = Vec::new();

    for (idx, facet) in facets.iter().enumerate() {
        if facet.index.is_empty() {
            continue;
        }
        for feature in &facet.features {
            events.push(FacetEvent::start(facet.index.start(), feature.clone(), idx));
            events.push(FacetEvent::end(facet.index.end(), feature.clone(), idx));
        }
    }

    events.sort();

    // Track active features in a stack: (feature, facet_idx)
    let mut active_stack: Vec<(FacetFeature<'a>, usize)> = Vec::new();
    let mut last_pos = 0;

    for event in events {
        let pos = event.pos.min(text.len());

        // Write text up to this position
        if pos > last_pos {
            if let Some(segment) = text.get(last_pos..pos) {
                output.write_text(segment)?;
            }
            last_pos = pos;
        }

        if event.is_start {
            output.start_feature(&event.feature)?;
            active_stack.push((event.feature, event.facet_idx));
        } else {
            // Find the feature in the stack that matches this end event
            let close_from = active_stack
                .iter()
                .rposition(|(f, idx)| *idx == event.facet_idx && feature_matches(f, &event.feature));

            if let Some(close_idx) = close_from {
                // Close features from top down to the one we need to close
                let to_reopen: Vec<_> = active_stack.drain(close_idx..).collect();

                // Close all features we're draining (in reverse order)
                for (f, _) in to_reopen.iter().rev() {
                    output.end_feature(f)?;
                }

                // Reopen features that aren't the one we're closing (skip first which is the one we're closing)
                for (f, idx) in to_reopen.into_iter().skip(1) {
                    output.start_feature(&f)?;
                    active_stack.push((f, idx));
                }
            }
        }
    }

    // Write remaining text
    if last_pos < text.len() {
        output.write_text(&text[last_pos..])?;
    }

    // Close any remaining open features
    for (feature, _) in active_stack.into_iter().rev() {
        output.end_feature(&feature)?;
    }

    Ok(())
}

fn feature_matches(a: &FacetFeature<'_>, b: &FacetFeature<'_>) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facet::types::ByteRange;

    struct TestOutput {
        buffer: String,
    }

    impl TestOutput {
        fn new() -> Self {
            Self {
                buffer: String::new(),
            }
        }
    }

    impl FacetOutput for TestOutput {
        type Error = std::fmt::Error;

        fn write_text(&mut self, text: &str) -> Result<(), Self::Error> {
            self.buffer.push_str(text);
            Ok(())
        }

        fn start_feature(&mut self, feature: &FacetFeature<'_>) -> Result<(), Self::Error> {
            match feature {
                FacetFeature::Bold => self.buffer.push_str("<b>"),
                FacetFeature::Italic => self.buffer.push_str("<i>"),
                FacetFeature::Link { uri } => {
                    self.buffer.push_str(&format!("<a href=\"{}\">", uri))
                }
                _ => self.buffer.push_str("<?>"),
            }
            Ok(())
        }

        fn end_feature(&mut self, feature: &FacetFeature<'_>) -> Result<(), Self::Error> {
            match feature {
                FacetFeature::Bold => self.buffer.push_str("</b>"),
                FacetFeature::Italic => self.buffer.push_str("</i>"),
                FacetFeature::Link { .. } => self.buffer.push_str("</a>"),
                _ => self.buffer.push_str("</?>"),
            }
            Ok(())
        }
    }

    #[test]
    fn test_simple_bold() {
        let text = "hello world";
        let facets = vec![NormalizedFacet {
            index: ByteRange::new(0, 5),
            features: vec![FacetFeature::Bold],
        }];

        let mut output = TestOutput::new();
        process_faceted_text(text, &facets, &mut output).unwrap();

        assert_eq!(output.buffer, "<b>hello</b> world");
    }

    #[test]
    fn test_overlapping_facets() {
        // "bold and italic just italic"
        //  ^^^^^^^^^^^^^   <- bold (0-13)
        //       ^^^^^^^^^^^^^^^^^^^^^^^ <- italic (5-27)
        let text = "bold and italic just italic";
        let facets = vec![
            NormalizedFacet {
                index: ByteRange::new(0, 15),
                features: vec![FacetFeature::Bold],
            },
            NormalizedFacet {
                index: ByteRange::new(5, 27),
                features: vec![FacetFeature::Italic],
            },
        ];

        let mut output = TestOutput::new();
        process_faceted_text(text, &facets, &mut output).unwrap();

        // Should properly nest: <b>bold <i>and italic</i></b><i> just italic</i>
        assert_eq!(
            output.buffer,
            "<b>bold <i>and italic</i></b><i> just italic</i>"
        );
    }

    #[test]
    fn test_no_facets() {
        let text = "plain text";
        let facets: Vec<NormalizedFacet> = vec![];

        let mut output = TestOutput::new();
        process_faceted_text(text, &facets, &mut output).unwrap();

        assert_eq!(output.buffer, "plain text");
    }

    #[test]
    fn test_link_facet() {
        let text = "click here for more";
        let facets = vec![NormalizedFacet {
            index: ByteRange::new(6, 10),
            features: vec![FacetFeature::Link {
                uri: "https://example.com",
            }],
        }];

        let mut output = TestOutput::new();
        process_faceted_text(text, &facets, &mut output).unwrap();

        assert_eq!(
            output.buffer,
            "click <a href=\"https://example.com\">here</a> for more"
        );
    }
}
