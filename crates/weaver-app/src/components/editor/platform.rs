//! Platform detection for browser-specific workarounds.
//!
//! Based on patterns from ProseMirror's input handling, adapted for Rust/wasm.

use std::sync::OnceLock;

/// Cached platform detection results.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Platform {
    pub ios: bool,
    pub mac: bool,
    pub android: bool,
    pub chrome: bool,
    pub safari: bool,
    pub gecko: bool,
    pub webkit_version: Option<u32>,
    pub chrome_version: Option<u32>,
    pub mobile: bool,
}

impl Default for Platform {
    fn default() -> Self {
        Self {
            ios: false,
            mac: false,
            android: false,
            chrome: false,
            safari: false,
            gecko: false,
            webkit_version: None,
            chrome_version: None,
            mobile: false,
        }
    }
}

static PLATFORM: OnceLock<Platform> = OnceLock::new();

/// Get cached platform info. Detection runs once on first call.
pub fn platform() -> &'static Platform {
    PLATFORM.get_or_init(detect_platform)
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn detect_platform() -> Platform {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return Platform::default(),
    };

    let navigator = window.navigator();
    let user_agent = navigator.user_agent().unwrap_or_default().to_lowercase();
    let platform_str = navigator.platform().unwrap_or_default().to_lowercase();

    // iOS detection: iPhone/iPad/iPod in UA, or Mac platform with touch
    let ios = user_agent.contains("iphone")
        || user_agent.contains("ipad")
        || user_agent.contains("ipod")
        || (platform_str.contains("mac") && has_touch_support(&navigator));

    // macOS (but not iOS)
    let mac = platform_str.contains("mac") && !ios;

    // Android
    let android = user_agent.contains("android");

    // Chrome (but not Edge, which also contains Chrome)
    let chrome = user_agent.contains("chrome") && !user_agent.contains("edg");

    // Safari (WebKit but not Chrome)
    let safari = user_agent.contains("safari") && !user_agent.contains("chrome");

    // Firefox/Gecko
    let gecko = user_agent.contains("gecko/") && !user_agent.contains("like gecko");

    // WebKit version extraction
    let webkit_version = extract_version(&user_agent, "applewebkit/");

    // Chrome version extraction
    let chrome_version = extract_version(&user_agent, "chrome/");

    // Mobile detection
    let mobile = ios || android || user_agent.contains("mobile") || user_agent.contains("iemobile");

    Platform {
        ios,
        mac,
        android,
        chrome,
        safari,
        gecko,
        webkit_version,
        chrome_version,
        mobile,
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn has_touch_support(navigator: &web_sys::Navigator) -> bool {
    // Check maxTouchPoints > 0 (indicates touch capability)
    navigator.max_touch_points() > 0
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn extract_version(ua: &str, prefix: &str) -> Option<u32> {
    ua.find(prefix).and_then(|idx| {
        let after = &ua[idx + prefix.len()..];
        // Take digits until non-digit
        let version_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        version_str.parse().ok()
    })
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn detect_platform() -> Platform {
    Platform::default()
}
