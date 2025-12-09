//! The views module contains the components for all Layouts and Routes for our app. Each layout and route in our [`Route`]
//! enum will render one of these components.
//!
//!
//! The [`Home`] and [`Blog`] components will be rendered when the current route is [`Route::Home`] or [`Route::Blog`] respectively.
//!
//!
//! The [`Navbar`] component will be rendered on all pages of our app since every page is under the layout. The layout defines
//! a common wrapper around all child routes.

mod home;
pub use home::Home;

mod navbar;
pub use navbar::Navbar;

mod notebookpage;
pub use notebookpage::NotebookPage;

mod notebook;
pub use notebook::{Notebook, NotebookIndex};

mod record;
pub use record::{RecordIndex, RecordPage, RecordView};

mod callback;
pub use callback::Callback;

mod editor;
pub use editor::Editor;

mod drafts;
pub use drafts::{DraftEdit, DraftsList, NewDraft, NotebookEntryEdit, StandaloneEntryEdit};

mod entry;
pub use entry::{NotebookEntryByRkey, StandaloneEntry};

mod invites;
pub use invites::InvitesPage;

mod footer;
pub use footer::Footer;

mod static_page;
pub use static_page::{AboutPage, PrivacyPage, TermsPage};
