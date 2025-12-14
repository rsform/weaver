//! com.atproto.identity.* endpoint handlers

use axum::{Json, extract::State};
use jacquard::IntoStatic;
use jacquard::types::ident::AtIdentifier;
use jacquard_axum::ExtractXrpc;
use weaver_api::com_atproto::identity::resolve_handle::{
    ResolveHandleOutput, ResolveHandleRequest,
};

use crate::endpoints::actor::resolve_actor;
use crate::endpoints::repo::XrpcErrorResponse;
use crate::server::AppState;

/// Handle com.atproto.identity.resolveHandle
pub async fn resolve_handle(
    State(state): State<AppState>,
    ExtractXrpc(args): ExtractXrpc<ResolveHandleRequest>,
) -> Result<Json<ResolveHandleOutput<'static>>, XrpcErrorResponse> {
    let did = resolve_actor(&state, &AtIdentifier::Handle(args.handle)).await?;

    Ok(Json(
        ResolveHandleOutput {
            did: did.into_static(),
            extra_data: None,
        }
        .into_static(),
    ))
}
