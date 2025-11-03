pub use weaver_api::sh_weaver::notebook::theme::{
    Theme, ThemeCodeTheme, ThemeColours, ThemeFonts, ThemeSpacing,
};
use weaver_common::jacquard::CowStr;
use weaver_common::jacquard::cowstr::ToCowStr;

pub fn default_theme() -> Theme<'static> {
    Theme::new()
        .code_theme(ThemeCodeTheme::CodeThemeName(Box::new(
            "rose-pine".to_cowstr(),
        )))
        .colours(ThemeColours {
            background: CowStr::new_static("#191724"),
            foreground: CowStr::new_static("#e0def4"),
            link: CowStr::new_static("#31748f"),
            link_hover: CowStr::new_static("#9ccfd8"),
            primary: CowStr::new_static("#c4a7e7"),
            secondary: CowStr::new_static("#3e8fb0"),

            ..Default::default()
        }).fonts(ThemeFonts {
            body: CowStr::new_static(
                "IBM Plex, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
            ),
            heading:CowStr::new_static(
                "IBM Plex Sans, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
            ),
            monospace: CowStr::new_static(
                "'IBM Plex Mono', 'Berkeley Mono', 'Cascadia Code', 'Roboto Mono', Consolas, monospace",
            ),
            ..Default::default()
        }).spacing(ThemeSpacing {
            base_size: CowStr::new_static("16px"),
            line_height: CowStr::new_static("1.6"),
            scale: CowStr::new_static("1.25"),
            ..Default::default()
        }).build()
}
