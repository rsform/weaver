//! Collaboration components for inviting and managing collaborators.

pub mod api;
mod avatars;
mod collaborators;
mod invite_dialog;
mod invites_list;

pub use api::{
    accept_invite, create_invite, fetch_received_invites, fetch_sent_invites, AcceptedInvite,
    ReceivedInvite, SentInvite,
};
pub use avatars::CollaboratorAvatars;
pub use collaborators::CollaboratorsPanel;
pub use invite_dialog::InviteDialog;
pub use invites_list::InvitesList;
