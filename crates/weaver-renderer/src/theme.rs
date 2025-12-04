use miette::IntoDiagnostic;
pub use weaver_api::sh_weaver::notebook::colour_scheme::{ColourScheme, ColourSchemeColours};
pub use weaver_api::sh_weaver::notebook::theme::{
    Theme, ThemeDarkCodeTheme, ThemeFonts, ThemeLightCodeTheme, ThemeSpacing,
};
use weaver_common::jacquard::CowStr;
use weaver_common::jacquard::IntoStatic;
use weaver_common::jacquard::client::AgentSession;
use weaver_common::jacquard::cowstr::ToCowStr;
use weaver_common::jacquard::prelude::*;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeDefault {
    #[default]
    Auto,
    Light,
    Dark,
}

/// A theme with resolved colour schemes (no strongRefs, actual data)
#[derive(Clone, Debug)]
pub struct ResolvedTheme<'a> {
    pub default: ThemeDefault,
    pub dark_scheme: ColourSchemeColours<'a>,
    pub light_scheme: ColourSchemeColours<'a>,
    pub fonts: ThemeFonts<'a>,
    pub spacing: ThemeSpacing<'a>,
    pub dark_code_theme: ThemeDarkCodeTheme<'a>,
    pub light_code_theme: ThemeLightCodeTheme<'a>,
}

pub fn default_colour_scheme_dark() -> ColourSchemeColours<'static> {
    ColourSchemeColours {
        base: CowStr::new_static("#191724"),
        surface: CowStr::new_static("#1f1d2e"),
        overlay: CowStr::new_static("#26233a"),
        text: CowStr::new_static("#e0def4"),
        muted: CowStr::new_static("#6e6a86"),
        subtle: CowStr::new_static("#908caa"),
        emphasis: CowStr::new_static("#e0def4"),
        primary: CowStr::new_static("#c4a7e7"),
        secondary: CowStr::new_static("#9ccfd8"),
        tertiary: CowStr::new_static("#ebbcba"),
        error: CowStr::new_static("#eb6f92"),
        warning: CowStr::new_static("#f6c177"),
        success: CowStr::new_static("#31748f"),
        border: CowStr::new_static("#403d52"),
        link: CowStr::new_static("#ebbcba"),
        highlight: CowStr::new_static("#524f67"),
        ..Default::default()
    }
}

pub fn default_fonts() -> ThemeFonts<'static> {
    ThemeFonts {
        // Serif for body text, sans for headings/UI
        body: CowStr::new_static(
            "'Adobe Caslon Pro', 'Latin Modern Roman',  'CM Serif', Georgia, serif",
        ),
        heading: CowStr::new_static(
            "'IBM Plex Sans', 'CM Sans','Junction', 'Proza Libre',   system-ui, sans-serif",
        ),
        monospace: CowStr::new_static(
            "'Ioskeley Mono', 'IBM Plex Mono', 'Berkeley Mono', Consolas, monospace",
        ),
        ..Default::default()
    }
}

pub fn default_spacing() -> ThemeSpacing<'static> {
    ThemeSpacing {
        base_size: CowStr::new_static("16px"),
        line_height: CowStr::new_static("1.6"),
        scale: CowStr::new_static("1.25"),
        ..Default::default()
    }
}

pub fn default_colour_scheme_light() -> ColourSchemeColours<'static> {
    // Rose Pine Dawn with moderate contrast text (text/muted/subtle/emphasis darkened)
    ColourSchemeColours {
        base: CowStr::new_static("#faf4ed"),
        surface: CowStr::new_static("#fffaf3"),
        overlay: CowStr::new_static("#f2e9e1"),
        // Text colors darkened for better contrast
        text: CowStr::new_static("#1f1d2e"),
        muted: CowStr::new_static("#635e74"),
        subtle: CowStr::new_static("#4a4560"),
        emphasis: CowStr::new_static("#1e1a2d"),
        // Accent colors kept at original Rose Pine Dawn values
        primary: CowStr::new_static("#907aa9"),
        secondary: CowStr::new_static("#56949f"),
        tertiary: CowStr::new_static("#286983"),
        error: CowStr::new_static("#b4637a"),
        warning: CowStr::new_static("#ea9d34"),
        success: CowStr::new_static("#286983"),
        border: CowStr::new_static("#dfdad9"),
        link: CowStr::new_static("#d7827e"),
        highlight: CowStr::new_static("#cecacd"),
        ..Default::default()
    }
}

pub fn default_resolved_theme() -> ResolvedTheme<'static> {
    ResolvedTheme {
        default: ThemeDefault::Auto,
        dark_scheme: default_colour_scheme_dark(),
        light_scheme: default_colour_scheme_light(),
        fonts: default_fonts(),
        spacing: default_spacing(),
        dark_code_theme: ThemeDarkCodeTheme::CodeThemeName(Box::new("rose-pine".to_cowstr())),
        light_code_theme: ThemeLightCodeTheme::CodeThemeName(Box::new(
            "rose-pine-dawn".to_cowstr(),
        )),
    }
}

/// Resolve a theme by fetching its colour scheme records from the PDS
pub async fn resolve_theme<A: AgentSession + IdentityResolver>(
    agent: &A,
    theme: &Theme<'_>,
) -> miette::Result<ResolvedTheme<'static>> {
    use weaver_common::jacquard::client::AgentSessionExt;

    // Fetch dark scheme
    let dark_response = agent
        .get_record::<ColourScheme>(&theme.dark_scheme.uri)
        .await
        .into_diagnostic()?;

    let dark_scheme: ColourScheme = dark_response.into_output().into_diagnostic()?.into();

    // Fetch light scheme
    let light_response = agent
        .get_record::<ColourScheme>(&theme.light_scheme.uri)
        .await
        .into_diagnostic()?;

    let light_scheme: ColourScheme = light_response.into_output().into_diagnostic()?.into();
    let default = match theme.default_theme.as_ref().map(|t| t.as_str()) {
        Some("auto") => ThemeDefault::Auto,
        Some("dark") => ThemeDefault::Dark,
        Some("light") => ThemeDefault::Light,
        _ => ThemeDefault::Auto,
    };

    Ok(ResolvedTheme {
        default,
        dark_scheme: dark_scheme.colours.into_static(),
        light_scheme: light_scheme.colours.into_static(),
        fonts: theme.fonts.clone().into_static(),
        spacing: theme.spacing.clone().into_static(),
        dark_code_theme: theme.dark_code_theme.clone().into_static(),
        light_code_theme: theme.light_code_theme.clone().into_static(),
    })
}
