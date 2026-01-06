//! Color utilities for editor UI.

/// Convert RGBA u32 (packed as 0xRRGGBBAA) to CSS rgba() string.
pub fn rgba_u32_to_css(color: u32) -> String {
    let r = (color >> 24) & 0xFF;
    let g = (color >> 16) & 0xFF;
    let b = (color >> 8) & 0xFF;
    let a = (color & 0xFF) as f32 / 255.0;
    format!("rgba({}, {}, {}, {})", r, g, b, a)
}

/// Convert RGBA u32 to CSS rgba() string with a custom alpha value.
///
/// Useful for creating semi-transparent versions of a color (e.g., selection highlights).
pub fn rgba_u32_to_css_alpha(color: u32, alpha: f32) -> String {
    let r = (color >> 24) & 0xFF;
    let g = (color >> 16) & 0xFF;
    let b = (color >> 8) & 0xFF;
    format!("rgba({}, {}, {}, {})", r, g, b, alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgba_to_css() {
        // Fully opaque red
        assert_eq!(rgba_u32_to_css(0xFF0000FF), "rgba(255, 0, 0, 1)");
        // Semi-transparent green
        assert_eq!(rgba_u32_to_css(0x00FF0080), "rgba(0, 255, 0, 0.5019608)");
        // Fully transparent blue
        assert_eq!(rgba_u32_to_css(0x0000FF00), "rgba(0, 0, 255, 0)");
    }

    #[test]
    fn test_rgba_to_css_alpha() {
        // Red with 25% alpha override
        assert_eq!(rgba_u32_to_css_alpha(0xFF0000FF, 0.25), "rgba(255, 0, 0, 0.25)");
    }
}
