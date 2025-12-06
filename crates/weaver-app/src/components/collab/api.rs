//! API functions for collaboration invites.

use crate::fetch::Fetcher;
use jacquard::IntoStatic;
use jacquard::prelude::*;
use jacquard::types::collection::Collection;
use jacquard::types::string::{AtUri, Cid, Datetime, Did, Nsid, RecordKey};
use jacquard::types::uri::Uri;
use reqwest::Url;
use std::collections::HashSet;
use weaver_api::com_atproto::repo::list_records::ListRecords;
use weaver_api::com_atproto::repo::strong_ref::StrongRef;
use weaver_api::sh_weaver::collab::{accept::Accept, invite::Invite};
use weaver_api::sh_weaver::notebook::entry::Entry;
use weaver_common::WeaverError;
use weaver_common::constellation::GetBacklinksQuery;

const ACCEPT_NSID: &str = "sh.weaver.collab.accept";
const CONSTELLATION_URL: &str = "https://constellation.microcosm.blue";

/// An invite sent by the current user.
#[derive(Clone, Debug, PartialEq)]
pub struct SentInvite {
    pub uri: AtUri<'static>,
    pub invitee: Did<'static>,
    pub resource_uri: AtUri<'static>,
    pub message: Option<String>,
    pub created_at: Datetime,
    pub accepted: bool,
}

/// An invite received by the current user.
#[derive(Clone, Debug, PartialEq)]
pub struct ReceivedInvite {
    pub uri: AtUri<'static>,
    pub cid: Cid<'static>,
    pub inviter: Did<'static>,
    pub resource_uri: AtUri<'static>,
    pub resource_cid: Cid<'static>,
    pub message: Option<String>,
    pub created_at: Datetime,
}

/// An accepted invite (for listing collaborators).
#[derive(Clone, Debug, PartialEq)]
pub struct AcceptedInvite {
    pub accept_uri: AtUri<'static>,
    pub collaborator: Did<'static>,
    pub resource_uri: AtUri<'static>,
    pub accepted_at: Datetime,
}

/// Create an invite to collaborate on a resource.
pub async fn create_invite(
    fetcher: &Fetcher,
    resource: StrongRef<'static>,
    invitee: Did<'static>,
    message: Option<String>,
) -> Result<AtUri<'static>, WeaverError> {
    let mut invite_builder = Invite::new()
        .resource(resource)
        .invitee(invitee)
        .created_at(Datetime::now());

    if let Some(msg) = message {
        invite_builder = invite_builder.message(Some(jacquard::CowStr::from(msg)));
    }

    let invite = invite_builder.build();

    let output = fetcher
        .create_record(invite, None)
        .await
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to create invite: {}", e)))?;

    Ok(output.uri.into_static())
}

/// Accept a collaboration invite.
pub async fn accept_invite(
    fetcher: &Fetcher,
    invite_ref: StrongRef<'static>,
    resource_uri: AtUri<'static>,
) -> Result<AtUri<'static>, WeaverError> {
    let accept = Accept::new()
        .invite(invite_ref)
        .resource(resource_uri)
        .created_at(Datetime::now())
        .build();

    let output = fetcher
        .create_record(accept, None)
        .await
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to accept invite: {}", e)))?;

    Ok(output.uri.into_static())
}

/// Fetch invites sent by the current user.
pub async fn fetch_sent_invites(fetcher: &Fetcher) -> Result<Vec<SentInvite>, WeaverError> {
    let did = fetcher
        .current_did()
        .await
        .ok_or_else(|| WeaverError::InvalidNotebook("Not authenticated".into()))?;

    let request = ListRecords::new()
        .repo(did)
        .collection(Nsid::raw(Invite::NSID))
        .limit(100)
        .build();

    let response = fetcher
        .send(request)
        .await
        .map_err(|e| WeaverError::InvalidNotebook(format!("Failed to list invites: {}", e)))?;

    let output = response.into_output().map_err(|e| {
        WeaverError::InvalidNotebook(format!("Failed to parse list response: {}", e))
    })?;

    let mut invites = Vec::new();
    for record in output.records {
        if let Ok(invite) = jacquard::from_data::<Invite>(&record.value) {
            let uri = record.uri.into_static();
            let accepted = check_invite_accepted(fetcher, &uri).await;

            invites.push(SentInvite {
                uri,
                invitee: invite.invitee.into_static(),
                resource_uri: invite.resource.uri.into_static(),
                message: invite.message.map(|s| s.to_string()),
                created_at: invite.created_at.clone(),
                accepted,
            });
        }
    }

    Ok(invites)
}

/// Check if an invite has been accepted by querying for accept records.
async fn check_invite_accepted(fetcher: &Fetcher, invite_uri: &AtUri<'_>) -> bool {
    let Ok(constellation_url) = Url::parse(CONSTELLATION_URL) else {
        return false;
    };

    // Query for sh.weaver.collab.accept records that reference this invite via .invite.uri
    let query = GetBacklinksQuery {
        subject: Uri::At(invite_uri.clone().into_static()),
        source: format!("{}:invite.uri", ACCEPT_NSID).into(),
        cursor: None,
        did: vec![],
        limit: 1,
    };

    let Ok(response) = fetcher.client.xrpc(constellation_url).send(&query).await else {
        return false;
    };

    let Ok(output) = response.into_output() else {
        return false;
    };

    !output.records.is_empty()
}

/// Fetch invites received by the current user (via Constellation backlinks).
///
/// This queries Constellation to find invite records where the current user
/// is the invitee, then fetches each record from the inviter's PDS to get
/// the full invite details.
pub async fn fetch_received_invites(fetcher: &Fetcher) -> Result<Vec<ReceivedInvite>, WeaverError> {
    let did = fetcher
        .current_did()
        .await
        .ok_or_else(|| WeaverError::InvalidNotebook("Not authenticated".into()))?;

    let constellation_url = Url::parse(CONSTELLATION_URL)
        .map_err(|e| WeaverError::InvalidNotebook(format!("Invalid constellation URL: {}", e)))?;

    // Query for sh.weaver.collab.invite records where .invitee = current user's DID
    let query = GetBacklinksQuery {
        subject: Uri::Did(did.clone()),
        source: format!("{}:invitee", Invite::NSID).into(),
        cursor: None,
        did: vec![],
        limit: 100,
    };

    let response = fetcher
        .client
        .xrpc(constellation_url)
        .send(&query)
        .await
        .map_err(|e| WeaverError::InvalidNotebook(format!("Constellation query failed: {}", e)))?;

    let output = response.into_output().map_err(|e| {
        WeaverError::InvalidNotebook(format!("Failed to parse constellation response: {}", e))
    })?;

    // For each RecordId, fetch the actual record from the inviter's PDS
    let mut invites = Vec::new();

    for record_id in output.records {
        let inviter_did = record_id.did.into_static();

        // Build the AT-URI for the invite record
        let uri_string = format!(
            "at://{}/{}/{}",
            inviter_did,
            Invite::NSID,
            record_id.rkey.as_ref()
        );
        let Ok(invite_uri) = AtUri::new(&uri_string) else {
            continue;
        };
        let invite_uri = invite_uri.into_static();

        // Fetch the invite record from the inviter's PDS
        let Ok(response) = fetcher.get_record::<Invite>(&invite_uri).await else {
            continue;
        };

        let Ok(record) = response.into_output() else {
            continue;
        };

        let Some(cid) = record.cid else {
            continue;
        };

        // record.value is already the typed Invite from get_record::<Invite>
        let invite = &record.value;

        invites.push(ReceivedInvite {
            uri: record.uri.into_static(),
            cid: cid.into_static(),
            inviter: inviter_did,
            resource_uri: invite.resource.uri.clone().into_static(),
            resource_cid: invite.resource.cid.clone().into_static(),
            message: invite.message.as_ref().map(|s| s.to_string()),
            created_at: invite.created_at.clone(),
        });
    }

    Ok(invites)
}

/// Find all participants (owner + collaborators) for a resource by its rkey.
///
/// This works regardless of which copy of the entry you're viewing because it
/// queries for invites by rkey pattern, then collects all involved DIDs.
pub async fn find_all_participants(
    fetcher: &Fetcher,
    resource_uri: &AtUri<'_>,
) -> Result<Vec<Did<'static>>, WeaverError> {
    let Some(rkey) = resource_uri.rkey() else {
        return Ok(vec![]);
    };

    let constellation_url = Url::parse(CONSTELLATION_URL)
        .map_err(|e| WeaverError::InvalidNotebook(format!("Invalid constellation URL: {}", e)))?;

    // Query for all invite records that reference entries with this rkey
    // We search for invites where resource.uri contains the rkey
    // The source pattern matches the JSON path in the invite record
    let query = GetBacklinksQuery {
        subject: Uri::At(resource_uri.clone().into_static()),
        source: format!("{}:resource.uri", Invite::NSID).into(),
        cursor: None,
        did: vec![],
        limit: 100,
    };

    let mut participants: HashSet<Did<'static>> = HashSet::new();

    // First try with the exact URI
    if let Ok(response) = fetcher.client.xrpc(constellation_url.clone()).send(&query).await {
        if let Ok(output) = response.into_output() {
            for record_id in &output.records {
                // The inviter (owner) is the DID that created the invite
                participants.insert(record_id.did.clone().into_static());

                // Now we need to fetch the invite to get the invitee
                let uri_string = format!(
                    "at://{}/{}/{}",
                    record_id.did,
                    Invite::NSID,
                    record_id.rkey.as_ref()
                );
                if let Ok(invite_uri) = AtUri::new(&uri_string) {
                    if let Ok(response) = fetcher.get_record::<Invite>(&invite_uri).await {
                        if let Ok(record) = response.into_output() {
                            let invite = &record.value;
                            // Check if this invite was accepted
                            if check_invite_accepted(fetcher, &invite_uri.into_static()).await {
                                participants.insert(invite.invitee.clone().into_static());
                            }
                        }
                    }
                }
            }
        }
    }

    // Also try querying with the owner's URI if we can determine it
    // This handles the case where we're viewing from a collaborator's copy
    let authority_did = match resource_uri.authority() {
        jacquard::types::ident::AtIdentifier::Did(d) => Some(d.clone().into_static()),
        _ => None,
    };

    if let Some(ref did) = authority_did {
        participants.insert(did.clone());
    }

    // If no participants found via invites, return just the current entry's authority
    if participants.is_empty() {
        if let Some(did) = authority_did {
            return Ok(vec![did]);
        }
    }

    Ok(participants.into_iter().collect())
}
