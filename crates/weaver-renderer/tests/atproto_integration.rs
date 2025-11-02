// Integration tests for AT Protocol rendering pipeline
//
// These tests verify the full markdown→markdown transformation pipeline:
// 1. Parse input markdown
// 2. Process through AtProtoPreprocessContext
// 3. Upload images to PDS
// 4. Canonicalize wikilinks and profile links
// 5. Write transformed markdown

// NOTE: Full implementation pending processor streaming support
// For now, these are placeholders that will be completed when:
// - NotebookProcessor can stream events through contexts
// - MarkdownWriter can consume event streams

#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn test_markdown_to_markdown_pipeline() {
        // TODO: Implement once processor streaming is available
        // This test should:
        // 1. Create mock vault with test markdown files
        // 2. Set up AtProtoPreprocessContext with test agent
        // 3. Process markdown through the pipeline
        // 4. Verify output contains canonical links
        // 5. Verify blob tracking captured image metadata
    }

    #[test]
    #[ignore]
    fn test_wikilink_canonicalization() {
        // TODO: Test that [[Entry Name]] becomes /{handle}/{notebook}/Entry_Name
    }

    #[test]
    #[ignore]
    fn test_image_upload_and_rewrite() {
        // TODO: Test that ![alt](./image.png) uploads blob and rewrites to /{notebook}/image/{name}
    }

    #[test]
    #[ignore]
    fn test_profile_link_resolution() {
        // TODO: Test that [[@handle]] resolves to /{handle}
    }
}
