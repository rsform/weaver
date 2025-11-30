use dioxus::prelude::*;
use dioxus_primitives::dialog::{
    self, DialogContentProps, DialogDescriptionProps, DialogRootProps, DialogTitleProps,
};

const DIALOG_CSS: Asset = asset!("./dialog.css");

/// Fixed-position overlay that works in both SSR and client-only modes.
/// Wraps dioxus_primitives DialogRoot with inline positioning styles
/// since the CSS file doesn't load reliably in client-only mode.
#[component]
pub fn DialogRoot(props: DialogRootProps) -> Element {
    let is_open = props.open.read().unwrap_or(false);

    let overlay_style = if is_open {
        "position: fixed; inset: 0; z-index: 1000; background: rgba(0,0,0,0.3); display: flex; align-items: center; justify-content: center;"
    } else {
        "display: none;"
    };

    rsx! {
        document::Link { rel: "stylesheet", href: DIALOG_CSS }
        div {
            style: "{overlay_style}",
            onclick: {
                let on_change = props.on_open_change.clone();
                move |_| {
                    on_change.call(false);
                }
            },
            dialog::DialogRoot {
                class: "dialog-backdrop-inner",
                id: props.id,
                is_modal: props.is_modal,
                open: props.open,
                default_open: props.default_open,
                on_open_change: props.on_open_change,
                attributes: props.attributes,
                {props.children}
            }
        }
    }
}

#[component]
pub fn DialogContent(props: DialogContentProps) -> Element {
    rsx! {
        div {
            onclick: move |e| e.stop_propagation(),
            dialog::DialogContent {
                class: "dialog",
                id: props.id,
                attributes: props.attributes,
                {props.children}
            }
        }
    }
}

#[component]
pub fn DialogTitle(props: DialogTitleProps) -> Element {
    rsx! {
        dialog::DialogTitle {
            class: "dialog-title",
            id: props.id,
            attributes: props.attributes,
            {props.children}
        }
    }
}

#[component]
pub fn DialogDescription(props: DialogDescriptionProps) -> Element {
    rsx! {
        dialog::DialogDescription {
            class: "dialog-description",
            id: props.id,
            attributes: props.attributes,
            {props.children}
        }
    }
}
