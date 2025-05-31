//! Atproto renderer
//!
//! This mode of the renderer renders either an entire notebook or entries in it to files suitable for inclusion
//! in a single-page app and uploads them to your Atproto PDS
//! It can be accessed via the appview at {your-handle}.weaver.sh/{notebook-name}.
//!
//! It can also be edited there.
//!
//! Link altering logic:
//!  - Option 1: leave (non-embed) links the same as in the markdown, have the CSM deal with them via some means
//!    such as adding "data-did" and "data-cid" attributes to the `<a/>` tag containing the DID and CID
//!    Pushes toward having the SPA/Appview do a bit more, but makes this step MUCH simpler
//!     - In this scenario, the rendering step can happen upon access (and then be cached) in the appview
//!     - More flexible in some ways, less in others
//!  - Option 2: alter links to point to other rendered blobs. Requires a certain amount of work to handle
//!    scenarios with a complex mesh of internal links, as the CID is altered by the editing of the link.
//!    Such cycles are handled in the simplest way, by rendering an absolute url which will make a call to the appview.
//!
