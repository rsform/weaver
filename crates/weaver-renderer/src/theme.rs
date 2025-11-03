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

/// A theme with resolved colour schemes (no strongRefs, actual data)
#[derive(Clone, Debug)]
pub struct ResolvedTheme<'a> {
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
        secondary: CowStr::new_static("#3e8fb0"),
        tertiary: CowStr::new_static("#9ccfd8"),
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
        body: CowStr::new_static(
            "IBM Plex, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
        ),
        heading: CowStr::new_static(
            "IBM Plex Sans, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
        ),
        monospace: CowStr::new_static(
            "'IBM Plex Mono', 'Berkeley Mono', 'Cascadia Code', 'Roboto Mono', Consolas, monospace",
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
    ColourSchemeColours {
        base: CowStr::new_static("#faf4ed"),
        surface: CowStr::new_static("#fffaf3"),
        overlay: CowStr::new_static("#f2e9e1"),
        text: CowStr::new_static("#575279"),
        muted: CowStr::new_static("#9893a5"),
        subtle: CowStr::new_static("#797593"),
        emphasis: CowStr::new_static("#575279"),
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

    Ok(ResolvedTheme {
        dark_scheme: dark_scheme.colours.into_static(),
        light_scheme: light_scheme.colours.into_static(),
        fonts: theme.fonts.clone().into_static(),
        spacing: theme.spacing.clone().into_static(),
        dark_code_theme: theme.dark_code_theme.clone().into_static(),
        light_code_theme: theme.light_code_theme.clone().into_static(),
    })
}
