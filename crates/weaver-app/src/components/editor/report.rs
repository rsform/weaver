//! Bug report dialog for the markdown editor.
//!
//! Captures editor state, DOM, and platform info for bug reports.
//! All capture happens on-demand when the report button is clicked.

use dioxus::prelude::*;

#[allow(unused_imports)]
use super::log_buffer;
#[allow(unused_imports)]
use super::storage::load_from_storage;

/// Captured report data.
#[derive(Clone, Default)]
struct ReportData {
    editor_text: String,
    dom_html: String,
    platform_info: String,
    recent_logs: String,
}

impl ReportData {
    /// Capture current state from DOM and LocalStorage.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    fn capture(editor_id: &str) -> Self {
        let dom_html = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id(editor_id))
            .map(|e| e.outer_html())
            .unwrap_or_default();

        let editor_text = load_from_storage("current")
            .map(|doc| doc.content())
            .unwrap_or_default();

        let platform_info = {
            let plat = weaver_editor_browser::platform();
            format!(
                "iOS: {}, Android: {}, Safari: {}, Chrome: {}, Firefox: {}, Mobile: {}\n\
                User Agent: {}",
                plat.ios,
                plat.android,
                plat.safari,
                plat.chrome,
                plat.gecko,
                plat.mobile,
                web_sys::window()
                    .and_then(|w| w.navigator().user_agent().ok())
                    .unwrap_or_default()
            )
        };

        let recent_logs = log_buffer::get_logs();

        Self {
            editor_text,
            dom_html,
            platform_info,
            recent_logs,
        }
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    fn capture(_editor_id: &str) -> Self {
        Self::default()
    }

    /// Generate mailto URL with report data.
    fn to_mailto(&self, email: &str, comment: &str) -> String {
        let subject = "Weaver Editor Bug Report";

        let body = format!(
            "## Bug Report\n\n\
            ### Comment\n{}\n\n\
            ### Platform Info\n```\n{}\n```\n\n\
            ### Recent Logs\n```\n{}\n```\n\n\
            ### Editor Text\n```markdown\n{}\n```\n\n\
            ### DOM State\n```html\n{}\n```",
            comment, self.platform_info, self.recent_logs, self.editor_text, self.dom_html
        );

        let encoded_subject = urlencoding::encode(subject);
        let encoded_body = urlencoding::encode(&body);

        format!(
            "mailto:{}?subject={}&body={}",
            email, encoded_subject, encoded_body
        )
    }
}

/// Props for the bug report button.
#[derive(Props, Clone, PartialEq)]
pub struct ReportButtonProps {
    /// Email address to send reports to.
    pub email: String,
    /// Editor element ID for DOM capture.
    pub editor_id: String,
}

/// Bug report button and dialog.
#[component]
pub fn ReportButton(props: ReportButtonProps) -> Element {
    let mut show_dialog = use_signal(|| false);
    let mut comment = use_signal(String::new);
    let mut report_data = use_signal(ReportData::default);

    let editor_id = props.editor_id.clone();
    let capture_state = move |_| {
        report_data.set(ReportData::capture(&editor_id));
        show_dialog.set(true);
    };

    let email = props.email.clone();
    let submit_report = move |_| {
        let data = report_data();
        #[allow(unused_variables)]
        let mailto_url = data.to_mailto(&email, &comment());

        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        if let Some(window) = web_sys::window() {
            let _ = window.open_with_url(&mailto_url);
        }

        show_dialog.set(false);
        comment.set(String::new());
    };

    let close_dialog = move |_| {
        show_dialog.set(false);
    };

    rsx! {
        button {
            class: "report-bug-button",
            onclick: capture_state,
            "Report Bug"
        }

        if show_dialog() {
            div {
                class: "report-dialog-overlay",
                role: "dialog",
                aria_modal: "true",
                aria_labelledby: "report-dialog-title",
                onclick: close_dialog,

                div {
                    class: "report-dialog",
                    onclick: move |e| e.stop_propagation(),

                    h2 { id: "report-dialog-title", "Report a Bug" }

                    div { class: "report-section",
                        label { "Describe the issue:" }
                        textarea {
                            class: "report-comment",
                            aria_label: "Describe the issue",
                            placeholder: "What happened? What did you expect?",
                            value: "{comment}",
                            oninput: move |e| comment.set(e.value()),
                            rows: "4",
                        }
                    }

                    details { class: "report-details",
                        summary { "Captured Data (click to expand)" }

                        div { class: "report-section",
                            h4 { "Platform" }
                            pre { "{report_data().platform_info}" }
                        }

                        div { class: "report-section",
                            h4 { "Recent Logs" }
                            pre { "{report_data().recent_logs}" }
                        }

                        div { class: "report-section",
                            h4 { "Editor Text" }
                            pre { "{report_data().editor_text}" }
                        }

                        div { class: "report-section",
                            h4 { "DOM HTML" }
                            pre { "{report_data().dom_html}" }
                        }
                    }

                    div { class: "report-actions",
                        button {
                            class: "report-cancel",
                            onclick: close_dialog,
                            "Cancel"
                        }
                        button {
                            class: "report-submit",
                            onclick: submit_report,
                            "Open Email"
                        }
                    }
                }
            }
        }
    }
}
