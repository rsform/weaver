use crate::Route;
use crate::auth::AuthState;
use crate::components::record_editor::EditableRecordContent;
use crate::components::record_view::{
    CodeView, PrettyRecordView, RecordViewLayout, SchemaView, ViewMode,
};
use crate::fetch::Fetcher;
use dioxus::prelude::*;
use jacquard::common::to_data;
use jacquard::smol_str::ToSmolStr;
use jacquard::{
    client::AgentSessionExt,
    common::IntoStatic,
    identity::lexicon_resolver::LexiconSchemaResolver,
    types::{aturi::AtUri, ident::AtIdentifier, string::Nsid},
};
use jacquard_lexicon::lexicon::LexiconDoc;

#[component]
pub fn RecordIndex() -> Element {
    let navigator = use_navigator();
    let mut uri_input = use_signal(|| String::new());
    let handle_uri_submit = move || {
        let input_uri = uri_input.read().clone();
        if !input_uri.is_empty() {
            if let Ok(parsed) = AtUri::new(&input_uri) {
                let link = format!("{}/record/{}", crate::env::WEAVER_APP_HOST, parsed);
                navigator.push(link);
            }
        }
    };
    rsx! {
        document::Stylesheet { href: asset!("/assets/styling/record-view.css") }
        div {
            class: "record-view-container",
            div { class: "record-header",
                h1 { "Record View" }
                div { class: "uri-input-section",
                    input {
                        r#type: "text",
                        class: "uri-input",
                        placeholder: "at://did:plc:.../collection/rkey",
                        value: "{uri_input}",
                        oninput: move |evt| uri_input.set(evt.value()),
                        onkeydown: move |evt| {
                            if evt.key() == Key::Enter {
                                handle_uri_submit();
                            }
                        },
                    }
                }
            }

            Outlet::<Route> {}
        }
    }
}

#[component]
pub fn RecordPage(uri: ReadSignal<Vec<String>>) -> Element {
    rsx! {
        {std::iter::once(rsx! {RecordView {uri}})}
    }
}

#[component]
pub fn RecordView(uri: ReadSignal<Vec<String>>) -> Element {
    let fetcher = use_context::<Fetcher>();
    info!("Uri:{:?}", uri().join("/"));
    let at_uri = AtUri::new_owned(&*uri.read().join("/"));
    if at_uri.is_err() {
        return rsx! {};
    }
    let uri = use_signal(move || AtUri::new_owned(&*uri.read().join("/")).unwrap());
    let mut view_mode = use_signal(|| ViewMode::Pretty);
    let mut edit_mode = use_signal(|| false);

    let client = fetcher.get_client();
    let record_resource = use_resource(move || {
        let client = client.clone();
        async move { client.fetch_record_slingshot(&*uri.read()).await }
    });

    // Fetch schema for the record
    let schema_resource = use_resource(move || {
        let fetcher = fetcher.clone();
        async move {
            let record_read = record_resource.read();
            let record = record_read.as_ref()?.as_ref().ok()?;

            let validator = jacquard_lexicon::validation::SchemaValidator::global();
            let main_type = record.value.type_discriminator();
            let mut main_schema = None;
            let mut resolved = std::collections::HashSet::new();

            // Helper to recursively resolve a schema and its refs
            fn resolve_schema_with_refs<'a>(
                fetcher: &'a Fetcher,
                type_str: &'a str,
                validator: &'a jacquard_lexicon::validation::SchemaValidator,
                resolved: &'a mut std::collections::HashSet<String>,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Option<LexiconDoc<'static>>> + 'a>,
            > {
                Box::pin(async move {
                    if resolved.contains(type_str) {
                        return None;
                    }
                    resolved.insert(type_str.to_string());

                    let mut split = type_str.split('#');
                    let nsid_str = split.next().unwrap_or_default();
                    let nsid = Nsid::new(nsid_str).ok()?;

                    let schema = fetcher.resolve_lexicon_schema(&nsid).await.ok()?;

                    // Register by base NSID only (validator handles fragment lookup)
                    validator
                        .registry()
                        .insert(nsid_str.to_smolstr(), schema.doc.clone());

                    // Find refs in the schema and resolve them
                    if let Ok(schema_data) = to_data(&schema.doc) {
                        for ref_val in schema_data.query("...ref").values() {
                            if let Some(ref_str) = ref_val.as_str() {
                                if ref_str.contains('.') {
                                    resolve_schema_with_refs(fetcher, ref_str, validator, resolved)
                                        .await;
                                }
                            }
                        }
                        for ref_val in schema_data.query("...refs").values() {
                            if let Some(ref_str) = ref_val.as_str() {
                                if ref_str.contains('.') {
                                    resolve_schema_with_refs(fetcher, ref_str, validator, resolved)
                                        .await;
                                }
                            }
                        }
                    }

                    Some(schema.doc)
                })
            }

            // Find and resolve all schemas (including main and nested)
            for type_val in record.value.query("...$type").values() {
                if let Some(type_str) = type_val.as_str() {
                    // Skip non-NSID types (like "blob")
                    if !type_str.contains('.') {
                        continue;
                    }

                    if let Some(schema) =
                        resolve_schema_with_refs(&fetcher, type_str, &validator, &mut resolved)
                            .await
                    {
                        // Keep the main record schema
                        if Some(type_str) == main_type {
                            main_schema = Some(schema);
                        }
                    }
                }
            }

            main_schema
        }
    });

    let schema_signal = use_memo(move || schema_resource.read().clone().flatten());

    // Check ownership for edit access
    let auth_state = use_context::<Signal<AuthState>>();
    let is_owner = use_memo(move || {
        let auth = auth_state();
        if !auth.is_authenticated() {
            return false;
        }

        // authority() returns &AtIdentifier which can be Did or Handle
        match &*uri.read().authority() {
            AtIdentifier::Did(record_did) => auth.did.as_ref() == Some(record_did),
            AtIdentifier::Handle(_) => {
                // Can't easily check ownership for handles without async resolution
                false
            }
        }
    });
    if let Some(Ok(record)) = &*record_resource.read() {
        let record_value = record.value.clone().into_static();
        let record = record.clone();

        rsx! {
            Fragment {  key: "{uri()}",
                RecordViewLayout {
                    uri: uri().clone(),
                    cid: record.cid.clone(),
                    schema: schema_signal,
                    record_value: record_value.clone(),
                    if edit_mode() {

                        EditableRecordContent {
                            record_value: record_value,
                            uri: uri,
                            view_mode: view_mode,
                            edit_mode: edit_mode,
                            record_resource: record_resource,
                            schema: schema_signal,
                        }
                    } else {
                        div {
                            class: "tab-bar",
                            button {
                                class: if view_mode() == ViewMode::Pretty { "tab-button active" } else { "tab-button" },
                                onclick: move |_| view_mode.set(ViewMode::Pretty),
                                "View"
                            }
                            button {
                                class: if view_mode() == ViewMode::Json { "tab-button active" } else { "tab-button" },
                                onclick: move |_| view_mode.set(ViewMode::Json),
                                "JSON"
                            }
                            button {
                                class: if view_mode() == ViewMode::Schema { "tab-button active" } else { "tab-button" },
                                onclick: move |_| view_mode.set(ViewMode::Schema),
                                "Schema"
                            }
                            if is_owner() {
                                button {
                                    class: "tab-button edit-button",
                                    onclick: move |_| edit_mode.set(true),
                                    "Edit"
                                }
                            }
                        }
                        div {
                            class: "tab-content",
                            match view_mode() {
                                ViewMode::Pretty => rsx! {
                                    PrettyRecordView { record: record_value, uri: uri().clone(), schema: schema_signal }
                                },
                                ViewMode::Json => {
                                    let json = use_memo(use_reactive!(|record| serde_json::to_string_pretty(
                                        &record.value
                                    )
                                    .unwrap_or_default()));
                                    rsx! {
                                        CodeView {
                                            code: json,
                                            lang: Some("json".to_string()),
                                        }
                                    }
                                },
                                ViewMode::Schema => rsx! {
                                    SchemaView { schema: schema_signal }
                                },
                            }
                        }
                    }
                }
            }
        }
    } else {
        rsx! {}
    }
}
