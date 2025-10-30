use smol_str::SmolStr;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Theme {
    pub colors: ColorScheme,
    pub fonts: FontScheme,
    pub spacing: SpacingScheme,
    pub syntect_theme_name: SmolStr,
    pub custom_syntect_theme_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ColorScheme {
    pub background: SmolStr,
    pub foreground: SmolStr,
    pub link: SmolStr,
    pub link_hover: SmolStr,
}

#[derive(Debug, Clone)]
pub struct FontScheme {
    pub body: SmolStr,
    pub heading: SmolStr,
    pub monospace: SmolStr,
}

#[derive(Debug, Clone)]
pub struct SpacingScheme {
    pub base_font_size: SmolStr,
    pub line_height: SmolStr,
    pub scale: SmolStr,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            colors: ColorScheme::default(),
            fonts: FontScheme::default(),
            spacing: SpacingScheme::default(),
            syntect_theme_name: SmolStr::new("rose-pine-dawn"),
            custom_syntect_theme_path: None,
        }
    }
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self {
            background: SmolStr::new("#faf4ed"),
            foreground: SmolStr::new("#2b303b"),
            link: SmolStr::new("#286983"),
            link_hover: SmolStr::new("#56949f"),
        }
    }
}

impl Default for FontScheme {
    fn default() -> Self {
        Self {
            body: SmolStr::new(
                "IBM Plex, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
            ),
            heading: SmolStr::new(
                "IBM Plex Sans, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
            ),
            monospace: SmolStr::new(
                "'IBM Plex Mono', 'Berkeley Mono', 'Cascadia Code', 'Roboto Mono', Consolas, monospace",
            ),
        }
    }
}

impl Default for SpacingScheme {
    fn default() -> Self {
        Self {
            base_font_size: SmolStr::new("16px"),
            line_height: SmolStr::new("1.6"),
            scale: SmolStr::new("1.25"),
        }
    }
}
