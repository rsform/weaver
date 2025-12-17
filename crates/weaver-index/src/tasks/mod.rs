//! Background tasks for the indexer

mod draft_titles;

pub use draft_titles::{run_draft_title_task, DraftTitleTaskConfig};
