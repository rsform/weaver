//! The components module contains all shared components for our app. Components are the building blocks of dioxus apps.
//! They can be used to defined common UI elements like buttons, forms, and modals. In this template, we define a Hero
//! component and an Echo component for fullstack apps to be used in our app.

pub mod css;
pub use css::NotebookCss;

mod entry;
pub use entry::{Entry, EntryCard};

pub mod identity;
pub use identity::{NotebookCard, Repository, RepositoryIndex};
pub mod avatar;

pub mod profile;
pub use profile::ProfileDisplay;

pub mod notebook_cover;
pub use notebook_cover::NotebookCover;
