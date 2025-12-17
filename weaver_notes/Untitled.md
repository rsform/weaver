I recently used Jacquard to write an ~AppView~ Index for Weaver. I alluded in my posts about my devlog about that experience how easy I had made the actual web server side of that. Lexicon as a specification language provides a lot of ways to specify data types and a few to specify API endpoints. XRPC is the canonical way to do that, and it's an opinionated subset of HTTP, which narrows down to a specific endpoint format and set of "verbs". Your path is `/xrpc/your.lexicon.nsidEndpoint?argument=value`, your bodies are mostly JSON. 

I'm going to lead off by tooting someone else's horn. Chad Miller's https://quickslice.slices.network/ provides an excellent example of the kind of thing you can do with atproto lexicons, and it doesn't use XRPC at all, but instead generates GraphQL's equivalents. This is more freeform, requires less of you upfront, and is in a lot of ways more granular than XRPC could possibly allow. Jacquard is for the moment built around the expectations of XRPC. If someone want's Jacquard support for GraphQL on atproto lexicons, I'm all ears, though.

Here's to me one of the benefits of XRPC, and one of the challenges. XRPC only specifies your inputs and your output. everything else between you need to figure out. This means more work, but it also means you have internal flexibility. And Jacquard's server-side XRPC helpers follow that. Jacquard XRPC code generation itself provides the output type and the errors. For the server side it generates one additional marker type, generally labeled `YourXrpcQueryRequest`, and a trait implementation for `XrpcEndpoint`. You can also get these with `derive(XrpcRequest)` on existing Rust structs without writing out lexicon JSON.

```rust
pub trait XrpcEndpoint {
    /// Fully-qualified path ('/xrpc/\[nsid\]') where this endpoint should live on the server
    const PATH: &'static str;
    /// XRPC method (query/GET or procedure/POST)
    const METHOD: XrpcMethod;
    /// XRPC Request data type
    type Request<'de>: XrpcRequest + Deserialize<'de> + IntoStatic;
    /// XRPC Response data type
    type Response: XrpcResp;
}

/// Endpoint type for
///sh.weaver.actor.getActorNotebooks
pub struct GetActorNotebooksRequest;
impl XrpcEndpoint for GetActorNotebooksRequest {
    const PATH: &'static str = "/xrpc/sh.weaver.actor.getActorNotebooks";
    const METHOD: XrpcMethod = XrpcMethod::Query;
    type Request<'de> = GetActorNotebooks<'de>;
    type Response = GetActorNotebooksResponse;
}
```

As with many Jacquard traits you see the associated types carrying the lifetime. You may ask, why a second struct and trait? This is very similar to the `XrpcRequest` trait, which is implemented on the request struct itself, after all.

```rust
impl<'a> XrpcRequest for GetActorNotebooks<'a> {
    const NSID: &'static str = "sh.weaver.actor.getActorNotebooks";
    const METHOD: XrpcMethod = XrpcMethod::Query;
    type Response = GetActorNotebooksResponse;
}
```

## Time for magic
The reason is that lifetime when combined with the constraints Axum puts on extractors.  Because the request type includes a lifetime, if we were to attempt to implement `FromRequest` directly for `XrpcRequest`, the trait would require that `XrpcRequest` be implemented for all lifetimes, and also apply an effective `DeserializeOwned` bound, even if we were to specify the `'static` lifetime as we do. And of course `XrpcRequest` is implemented for one specific lifetime, `'a`, the lifetime of whatever it's borrowed from. Meanwhile `XrpcEndpoint` has no lifetime itself, but instead carries the lifetime on the `Request` associated type. This allows us to do the following implementation, where `ExtractXrpc<E>` has no lifetime itself and contains an owned version of the deserialized request. And we can then implement `FromRequest` for `ExtractXrpc<R>`, and put the `for<'any>` bound on the `IntoStatic` trait requirement in a where clause, where it works perfectly. In combination with the code generation in `jacquard-lexicon`, this is the full implementation of Jacquard's Axum XRPC request extractor. Not so bad.

```rust
pub struct ExtractXrpc<E: XrpcEndpoint>(pub E::Request<'static>);

impl<S, R> FromRequest<S> for ExtractXrpc<R>
where
    S: Send + Sync,
    R: XrpcEndpoint,
    for<'a> R::Request<'a>: IntoStatic<Output = R::Request<'static>>,
{
    type Rejection = Response;

    fn from_request(
        req: Request,
        state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
	    async {
            match R::METHOD {
                XrpcMethod::Procedure(_) => {
                    let body = Bytes::from_request(req, state)
                        .await
                        .map_err(IntoResponse::into_response)?;
                    let decoded = R::Request::decode_body(&body);
                    match decoded {
                        Ok(value) => Ok(ExtractXrpc(*value.into_static())),
                        Err(err) => Err((
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "error": "InvalidRequest",
                                "message": format!("failed to decode request: {}", err)
                            })),
                        ).into_response()),
                    }
                }
                XrpcMethod::Query => {
                    if let Some(path_query) = req.uri().path_and_query() {
                        let query = path_query.query().unwrap_or("");
                        let value: R::Request<'_> =
                            serde_html_form::from_str::<R::Request<'_>>(query).map_err(|e| {
                                (
                                    StatusCode::BAD_REQUEST,
                                    Json(json!({
                                        "error": "InvalidRequest",
                                        "message": format!("failed to decode request: {}", e)
                                    })),
                                ).into_response()
                            })?;
                        Ok(ExtractXrpc(value.into_static()))
                    } else {
                        Err((
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "error": "InvalidRequest",
                                "message": "wrong path"
                            })),
                        ).into_response())
                    }
                }
            }
        }
    }
```

Jacquard then also provides an additional utility to round things out, using the associated `PATH` constant to put the handler for your XRPC request at the right spot in your router.
```rust
/// Conversion trait to turn an XrpcEndpoint and a handler into an axum Router
pub trait IntoRouter {
    fn into_router<T, S, U>(handler: U) -> Router<S>
    where
        T: 'static,
        S: Clone + Send + Sync + 'static,
        U: axum::handler::Handler<T, S>;
}

impl<X> IntoRouter for X
where
    X: XrpcEndpoint,
{
    /// Creates an axum router that will invoke `handler` in response to xrpc
    /// request `X`.
    fn into_router<T, S, U>(handler: U) -> Router<S>
    where
        T: 'static,
        S: Clone + Send + Sync + 'static,
        U: axum::handler::Handler<T, S>,
    {
        Router::new().route(
            X::PATH,
            (match X::METHOD {
                XrpcMethod::Query => axum::routing::get,
                XrpcMethod::Procedure(_) => axum::routing::post,
            })(handler),
        )
    }
}
```

Which then lets the Axum router for Weaver's Index look like this (truncated for length):

```rust
pub fn router(state: AppState, did_doc: DidDocument<'static>) -> Router {
    Router::new()
        .route("/", get(landing))
        .route(
            "/assets/IoskeleyMono-Regular.woff2",
            get(font_ioskeley_regular),
        )
        .route("/assets/IoskeleyMono-Bold.woff2", get(font_ioskeley_bold))
        .route(
            "/assets/IoskeleyMono-Italic.woff2",
            get(font_ioskeley_italic),
        )
        .route("/xrpc/_health", get(health))
        .route("/metrics", get(metrics))
        // com.atproto.identity.* endpoints
        .merge(ResolveHandleRequest::into_router(identity::resolve_handle))
        // com.atproto.repo.* endpoints (record cache)
        .merge(GetRecordRequest::into_router(repo::get_record))
        .merge(ListRecordsRequest::into_router(repo::list_records))
        // app.bsky.* passthrough endpoints
        .merge(BskyGetProfileRequest::into_router(bsky::get_profile))
        .merge(BskyGetPostsRequest::into_router(bsky::get_posts))
        // sh.weaver.actor.* endpoints
        .merge(GetProfileRequest::into_router(actor::get_profile))
        .merge(GetActorNotebooksRequest::into_router(
            actor::get_actor_notebooks,
        ))
        .merge(GetActorEntriesRequest::into_router(
            actor::get_actor_entries,
        ))
        // sh.weaver.notebook.* endpoints
        ...
        // sh.weaver.collab.* endpoints
        ...
        // sh.weaver.edit.* endpoints
        ...
        .layer(TraceLayer::new_for_http())
		.layer(CorsLayer::permissive()
			.max_age(std::time::Duration::from_secs(86400))
		).with_state(state)
        .merge(did_web_router(did_doc))
}
```

Each of the handlers is a fairly straightforward async function that takes `AppState`, the XrpcExtractor, and an extractor and validator for service auth, which allows it to be accessed through via your PDS via the `atproto-proxy` header, and return user-specific data, or gate specific endpoints as requiring authentication.

> And so yeah, the actual HTTP server part of the index was dead-easy to write. The handlers themselves are some of them fairly *long* functions, as they need to pull together the required data from the database over a couple of queries and then do some conversion, but they're straightforward. At some point I may end up either adding additional specialized view tables to the database or rewriting my queries to do more in SQL or both, but for now it made sense to keep the final decision-making and assembly in Rust, where it's easier to iterate on.
### Service Auth
Service Auth is, for those not familiar, the non-OAuth way to talk to an XRPC server other than your PDS with an authenticated identity. It's the method the Bluesky AppView uses. There are some downsides to proxying through the PDS, like delay in being able to read your own writes without some PDS-side or app-level handling, but it is conceptually very simple. The PDS, when it pipes through an XRPC request to another service, validates authentication, then generates a short-lived JWT, signs it with the user's private key, and puts it in a header. The service then extracts that, decodes it, and validates it using the public key in the user's DID document. Jacquard provides a middleware that can be used to gate routes based on service auth validation and it also provides an extractor. Initially I provided just one where authentication is required, but as part of building the index I added an additional one for optional authentication, where the endpoint is public, but returns user-specific information when there is an authenticated user. It returns this structure.

```rust
#[derive(Debug, Clone, jacquard_derive::IntoStatic)]
pub struct VerifiedServiceAuth<'a> {
    /// The authenticated user's DID (from `iss` claim)
    did: Did<'a>,
    /// The audience (should match your service DID)
    aud: Did<'a>,
    /// The lexicon method NSID, if present
    lxm: Option<Nsid<'a>>,
    /// JWT ID (nonce), if present
    jti: Option<CowStr<'a>>,
}
```

Ultimately I want to provide a similar set of OAuth extractors as well, but those need to be built, still. If I move away from service proxying for the Weaver index, they will definitely get written at that point.

> I mentioned some bug-fixing in Jacquard was required to make this work. There were a couple of oversights in the `DidDocument` struct and a spot I had incorrectly held a tracing span across an await point. Also, while using the `slingshot_resolver` set of options for `JacquardResolver` is great under normal circumstances (and normally I default to it), the mini-doc does NOT in fact include the signing keys, and cannot be used to validate service auth. 
> 
> I am not always a smart woman.

## Why not go full magic?
One thing the Jacquard service auth validation extractor does **not** provide is validation of that jti nonce. That is left as an exercise for the server developer, to maintain a cache of recent nonces and compare against them. I leave a number of things this way, and this is deliberate. I think this is the correct approach. As powerful as "magic" all-in-one frameworks like Dioxus (or the various full-stack JS frameworks) are, the magic usually ends up constraining you in a number of ways. There are a number of awkward things in the front-end app implementation which are downstream of constraints Dioxus applies to your types and functions in order to work its magic.

There are a lot of possible things you might want to do as an XRPC server. You might be a PDS, you might be an AppView or index, you might be some other sort of service that doesn't really fit into the boxes (like a Tangled knot server or Streamplace node) you might authenticate via service auth or OAuth, communicate via the PDS or directly with the client app. And as such, while my approach to everything in Jacquard is to provide a comprehensive box of tools rather than a complete end-to-end solution, this is especially true on the server side of things, because of that diversity in requirements, and my desire to not constrain developers using the library to work a certain way, so that they can build anything they want on atproto.

> If you haven't read the Not An AppView entry, here it is. I might recommend reading it, and some other previous entries in that notebook, as it will help put the following in context. 
 
![[at://did:plc:yfvwmnlztr4dwkb7hwz55r2g/sh.weaver.notebook.entry/3m7ysqf2z5s22]]
## Dogfooding again
That being said, my experience writing the Weaver front-end and now the index server does leave me wanting a few things. One is a "BFF" session type, which forwards requests through a server to the PDS (or index), acting somewhat like [oatproxy](https://github.com/streamplace/oatproxy) (prototype jacquard version of that [here](https://github.com/espeon/istat/tree/main/jacquard-oatproxy) courtesy of Nat and Claude). This allows easier reading of your own writes via server-side caching, some caching and deduplication of common requests to reduce load on the PDS and roundtrip time. If the seession lives server-side it allows longer-lived confidential sessions for OAuth, and avoids putting OAuth tokens on the client device. 

Once implemented, I will likely refactor the Weaver app to use this session type in fullstack-server mode, which will then help dramatically simplify a bunch of client-side code. The refactored app will likely include an internal XRPC "server" of sorts that will elide differences between the index's XRPC APIs and the index-less flow. With the "fullstack-server" and "use-index" features, the client app running in the browser will forward authenticated requests through the app server to the index or PDS. With "fullstack-server" only, the app server itself acts like a discount version of the index, implemented via generic services like Constellation. Performance will be significantly improved over the original index-less implementation due to better caching, and unifying the cache. In client-only mode there are a couple of options, and I am not sure which is ultimately correct. The straightforward way as far as separation of concerns goes would be to essentially use a web worker as intermediary and local cache. That worker would be compiled to either use the index or to make Constellation and direct PDS requests, depending on the "use-index" feature. However that brings with it the obvious overhead of copying data from the worker to the app in the default mode, and I haven't yet investigated how feasible the available options which might allow zero-copy transfer via SharedArrayBuffer are. That being said, the real-time collaboration feature already works this way (sans SharedArrayBuffer) and lag is comparable to when the iroh connection was handled in the UI thread.

A fair bit of this is somewhat new territory for me, when it comes to the browser, and I would be ***very*** interested in hearing from people with more domain experience on the likely correct approach. 

On that note, one of my main frustrations with Jacquard as a library is how heavy it is in terms of compiled binary size due to monomorphization. I made that choice, to do everything via static dispatch, but when you want to ship as small a binary as possible over the network, it works against you. On WASM I haven't gotten a great number of exactly the granular damage, but on x86_64 (albeit with less aggressive optimisation for size) we're talking kilobytes of pure duplicated functions for every jacquard type used in the application, plus whatever else.
```rust
0.0%   0.0%  9.3KiB        weaver_app weaver_app::components::editor::sync::create_diff::{closure#0}
0.0%   0.0%  9.2KiB     loro_internal <loro_internal::txn::Transaction>::_commit
0.0%   0.0%  9.2KiB        weaver_app <weaver_app::fetch::Fetcher as jacquard::client::AgentSessionExt>::get_record::<weaver_api::sh_weaver::collab::invite::Invite>::{closure#0}
0.0%   0.0%  9.2KiB        weaver_app <weaver_app::fetch::Fetcher as jacquard::client::AgentSessionExt>::get_record::<weaver_api::sh_weaver::actor::profile::ProfileRecord>::{closure#0}
0.0%   0.0%  9.2KiB        weaver_app <weaver_app::fetch::Fetcher as jacquard::client::AgentSessionExt>::get_record::<weaver_api::app_bsky::actor::profile::ProfileRecord>::{closure#0}
0.0%   0.0%  9.2KiB   weaver_renderer <jacquard_identity::JacquardResolver as jacquard_identity::resolver::IdentityResolver>::resolve_did_doc::{closure#0}::{closure#0}
0.0%   0.0%  9.2KiB        weaver_app <weaver_app::fetch::Client as jacquard::client::AgentSessionExt>::get_record::<weaver_api::sh_weaver::notebook::theme::Theme>::{closure#0}
0.0%   0.0%  9.2KiB        weaver_app <weaver_app::fetch::Client as jacquard::client::AgentSessionExt>::get_record::<weaver_api::sh_weaver::notebook::entry::Entry>::{closure#0}
0.0%   0.0%  9.2KiB        weaver_app <weaver_app::fetch::Client as jacquard::client::AgentSessionExt>::get_record::<weaver_api::sh_weaver::notebook::book::Book>::{closure#0}
0.0%   0.0%  9.2KiB        weaver_app <weaver_app::fetch::Client as jacquard::client::AgentSessionExt>::get_record::<weaver_api::sh_weaver::notebook::colour_scheme::ColourScheme>::{closure#0}
0.0%   0.0%  9.2KiB        weaver_app <weaver_app::fetch::Client as jacquard::client::AgentSessionExt>::get_record::<weaver_api::sh_weaver::actor::profile::ProfileRecord>::{closure#0}
0.0%   0.0%  9.2KiB        weaver_app <weaver_app::fetch::Client as jacquard::client::AgentSessionExt>::get_record::<weaver_api::sh_weaver::edit::draft::Draft>::{closure#0}
0.0%   0.0%  9.2KiB        weaver_app <weaver_app::fetch::Client as jacquard::client::AgentSessionExt>::get_record::<weaver_api::sh_weaver::edit::root::Root>::{closure#0}
0.0%   0.0%  9.2KiB        weaver_app <weaver_app::fetch::Client as jacquard::client::AgentSessionExt>::get_record::<weaver_api::sh_weaver::edit::diff::Diff>::{closure#0}
0.0%   0.0%  9.2KiB        weaver_app <weaver_app::fetch::Client as jacquard::client::AgentSessionExt>::get_record::<weaver_api::app_bsky::actor::profile::ProfileRecord>::{closure#0}
0.0%   0.0%  9.2KiB             resvg <image_webp::vp8::Vp8Decoder<std::io::Take<&mut std::io::cursor::Cursor<&[u8]>>>>::loop_filter
0.0%   0.0%  9.2KiB            miette <miette::handlers::graphical::GraphicalReportHandler>::render_context::<alloc::string::String>
0.0%   0.0%  9.1KiB            miette <miette::handlers::graphical::GraphicalReportHandler>::render_context::<core::fmt::Formatter>
0.0%   0.0%  9.1KiB        weaver_app weaver_app::components::record_editor::EditableRecordContent::{closure#7}::{closure#0}
```

I've taken a couple stabs at refactors to help with this, but haven't found a solution that satisfies me, in part because one of the problems in practice is of course overhead from `serde_json` monomorphization. Unfortunately, the alternatives trade off in frustrating ways. [`facet`](https://github.com/facet-rs/facet) has its own binary size impacts and `facet-json` is missing a couple of critical features to work with atproto JSON data (internally-tagged enums, most notably). Something like [`simd-json`](https://github.com/simd-lite/simd-json) or [`serde_json_borrow`](https://github.com/PSeitz/serde_json_borrow) is fast and can borrow from the buffer in a way that is very useful to us (and honestly I intend to swap to them for some uses at some point), but `serde_json_borrow` only provides a value type, and I would then be uncertain at the monomorphization overhead of transforming that type into jacquard types. The `serde` implementation for `simd-json` is heavily based on `serde_json` and thus likely has much the same overhead problem. And [`miniserde`](https://github.com/dtolnay/miniserde) similarly lacks support for parts of JSON that atproto data requires (enums again). And writing my own custom JSON parser that deserializes into Jacquard's `Data` or `RawData` types (from where it can then be deserialized more simply into concrete types, ideally with much less code duplication) is not a project I have time for, and is on the tedious side of the kind of thing I enjoy, particularly the process of ensuring it is sufficiently robust for real-world use, and doesn't perform terribly.

`dyn` compatibility for some of the Jacquard traits is possible but comes with its own challenges, as currently `Serialize` is a supertrait of `XrpcRequest`, and rewriting around removing that bound that is both a nontrivial refactor (and a breaking API change, and it's not the only barrier to dyn compatibility) and may not actually reduce the number of copies of `get_record()` in the binary as much as one would hope. Now, if most of the code could be taken out of that and put into a function that could be totally shared between all implementations or at least most, that would be ideal but the solution I found prevented the compiler from inferring the output type from the request type, it decoupled those two things too much. Obviously if I were to do a bunch of cursed internal unsafe rust I could probably make this work, but while I'm comfortable writing unsafe Rust I'm also conscious that I'm writing Jacquard not just for myself. My code will run in situations I cannot anticipate, and it needs to be as reliable as possible and as usable as possible. Additional use of unsafe could help with the latter (laundering lifetimes would make a number of things in Jacquard's main code paths much easier, both for me and for users of the library) but at potential cost to the former if I'm not smart enough or comprehensive enough in my testing.

So I leave you, dear reader, with some questions this time. 

What choices make sense here? For Jacquard as a library, for writing web applications in Rust, and so on. I'm pretty damn good at this (if I do say so myself, and enough other people agree that I must accept it), but I'm also one person, with a necessarily incomplete understanding of the totality of the field.