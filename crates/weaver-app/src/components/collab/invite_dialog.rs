//! Dialog for inviting collaborators.

use crate::components::button::{Button, ButtonVariant};
use crate::components::dialog::{DialogContent, DialogDescription, DialogRoot, DialogTitle};
use crate::components::input::Input;
use crate::fetch::Fetcher;
use dioxus::prelude::*;
use jacquard::smol_str::format_smolstr;
use jacquard::types::string::{AtUri, Cid, Handle};
use jacquard::{IntoStatic, prelude::*};
use weaver_api::com_atproto::repo::strong_ref::StrongRef;

use super::api::create_invite;

/// Props for the InviteDialog component.
#[derive(Props, Clone, PartialEq)]
pub struct InviteDialogProps {
    /// Whether the dialog is open.
    pub open: bool,
    /// Callback when dialog should close.
    pub on_close: EventHandler<()>,
    /// The resource to invite collaborators to.
    pub resource_uri: AtUri<'static>,
    /// The CID of the resource.
    pub resource_cid: String,
    /// Optional title of the resource for display.
    #[props(default)]
    pub resource_title: Option<String>,
}

/// Dialog for inviting a user to collaborate on a resource.
#[component]
pub fn InviteDialog(props: InviteDialogProps) -> Element {
    let fetcher = use_context::<Fetcher>();
    let mut handle_input = use_signal(String::new);
    let mut message_input = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut is_sending = use_signal(|| false);

    let resource_uri = props.resource_uri.clone();
    let resource_cid = props.resource_cid.clone();
    let on_close = props.on_close.clone();

    let send_invite = move |_| {
        let fetcher = fetcher.clone();
        let handle = handle_input();
        let message = message_input();
        let resource_uri = resource_uri.clone();
        let resource_cid = resource_cid.clone();
        let on_close = on_close.clone();

        spawn(async move {
            is_sending.set(true);
            error.set(None);

            // Parse and resolve handle to DID
            let handle = match Handle::new(&handle) {
                Ok(h) => h,
                Err(e) => {
                    error.set(Some(format_smolstr!("Invalid handle: {}", e).into()));
                    is_sending.set(false);
                    return;
                }
            };

            let invitee_did = match fetcher.resolve_handle(&handle).await {
                Ok(did) => did,
                Err(e) => {
                    error.set(Some(format_smolstr!("Could not resolve handle: {}", e).into()));
                    is_sending.set(false);
                    return;
                }
            };

            // Build the resource StrongRef
            let cid = match Cid::new(resource_cid.as_bytes()) {
                Ok(c) => c.into_static(),
                Err(e) => {
                    error.set(Some(format_smolstr!("Invalid CID: {}", e).into()));
                    is_sending.set(false);
                    return;
                }
            };

            let resource_ref = StrongRef::new().uri(resource_uri).cid(cid).build();

            let message_opt = if message.is_empty() {
                None
            } else {
                Some(message)
            };

            match create_invite(
                &fetcher,
                resource_ref,
                invitee_did.into_static(),
                message_opt,
            )
            .await
            {
                Ok(_uri) => {
                    // Success - close dialog
                    handle_input.set(String::new());
                    message_input.set(String::new());
                    on_close.call(());
                }
                Err(e) => {
                    error.set(Some(format_smolstr!("Failed to send invite: {}", e).into()));
                }
            }

            is_sending.set(false);
        });
    };

    let resource_display = props
        .resource_title
        .clone()
        .unwrap_or_else(|| props.resource_uri.to_string());

    rsx! {
        DialogRoot {
            open: props.open,
            on_open_change: move |open: bool| {
                if !open {
                    props.on_close.call(());
                }
            },
            DialogContent {
                DialogTitle { "Invite Collaborator" }
                DialogDescription {
                    "Invite someone to collaborate on {resource_display}"
                }

                div { class: "invite-form",
                    div { class: "form-field",
                        label { "User handle" }
                        Input {
                            value: handle_input(),
                            placeholder: "user.bsky.social",
                            oninput: move |e: FormEvent| handle_input.set(e.value()),
                        }
                    }

                    div { class: "form-field",
                        label { "Message (optional)" }
                        textarea {
                            class: "invite-message",
                            value: "{message_input}",
                            placeholder: "Add a message...",
                            oninput: move |e| message_input.set(e.value()),
                            rows: 3,
                        }
                    }

                    if let Some(err) = error() {
                        div { class: "error-message", "{err}" }
                    }

                    div { class: "dialog-actions",
                        Button {
                            variant: ButtonVariant::Primary,
                            onclick: send_invite,
                            disabled: is_sending() || handle_input().is_empty(),
                            if is_sending() { "Sending..." } else { "Send Invite" }
                        }
                        Button {
                            variant: ButtonVariant::Ghost,
                            onclick: move |_| props.on_close.call(()),
                            "Cancel"
                        }
                    }
                }
            }
        }
    }
}
