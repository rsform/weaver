
If you've been to this site before, you maybe noticed it loaded a fair bit more quickly this time. That's not really because the web server creating this HTML got a whole lot better. It did require some refactoring, but it was mostly in the vein of taking some code and adding new code that did the same thing gated behind a cargo feature. This did, however, have the side effect of, in the final binary, replacing functions that are literally hundreds of lines, that in turn call functions that may also be hundreds of lines, making several cascading network requests, with functions that look like this, which make by and large a single network request and return exactly what is required.

```rust
#[cfg(feature = "use-index")]
fn fetch_entry_view(
	&self,
	entry_ref: &StrongRef<'_>,
) -> impl Future<Output = Result<EntryView<'static>, WeaverError>>
where
	Self: Sized,
{
	async move {
		use weaver_api::sh_weaver::notebook::get_entry::GetEntry;

		let resp = self
			.send(GetEntry::new().uri(entry_ref.uri.clone()).build())
			.await
			.map_err(|e| AgentError::from(ClientError::from(e)))?;

		let output = resp.into_output().map_err(|e| {
			AgentError::xrpc(e.into))
		})?;

		Ok(output.value.into_static())
	}
}
```

Of course the reason is that I finally got round to building the Weaver AppView. I'm going to be calling mine the Index, because Weaver is about writing and I think "AppView" as a term kind of sucks and "index" is much more elegant, on top of being a good descriptor of what the big backend service now powering Weaver does. ![[at://did:plc:ragtjsm2j2vknwkz3zp4oxrd/app.bsky.feed.post/3lyucxfxq622w]]
For the uninitiated, because I expect at least some people reading this aren't big into AT Protocol development, an AppView is an instance of the kind of big backend service that Bluesky PBLLC runs which powers essentially every Bluesky client, with a few notable exceptions, such as [Red Dwarf](https://reddwarf.app/), and (partially, eventually more completely) [Blacksky](https://blacksky.community/). It listens to the [Firehose](https://bsky.network/) [event stream](https://atproto.com/specs/event-stream) from the main Bluesky Relay and analyzes the data which comes through that pertains to Bluesky, producing your timeline feeds, figuring out who follows you, who you block and who blocks you (and filtering them out of your view of the app), how many people liked your last post, and so on. Because the records in your PDS (and those of all the other people on Bluesky) need context and relationship and so on to give them meaning, and then that context can be passed along to you without your app having to go collect it all. ![[at://did:plc:uu5axsmbm2or2dngy4gwchec/app.bsky.feed.post/3lsc2tzfsys2f]]
It's a very normal backend with some weird constraints because of the protocol, and in it's practice the thing that separates the day-to-day Bluesky experience from the Mastodon experience the most. It's also by far the most centralising force in the network, because it also does moderation, and because it's quite expensive to run. A full index of all Bluesky activity takes a lot of storage (futur's Zeppelin experiment detailed above took about 16 terabytes of storage using PostgreSQL for the database and cost $200/month to run), and then it takes that much more computing power to calculate all the relationships between the data on the fly as new events come in and then serve personalized versions to everyone that uses it.



It's not the only AppView out there, most atproto apps have something like this. Tangled, Streamplace, Leaflet, and so on all have substantial backends. Some (like Tangled) actually combine the front end you interact with and the AppView into a single service. But in general these are big, complicated persistent services you have to backfill from existing data to bootstrap, and they really strongly shape your app, whether they're literally part of the same executable or hosted on the same server or not. And when I started building Weaver in earnest, not only did I still have a few big unanswered questions about how I wanted Weaver to work, how it needed to work, I also didn't want to fundamentally tie it to some big server, create this centralising force. I wanted it to be possible for someone else to run it without being dependent on me personally, ideally possible even if all they had access to was a static site host like GitHub Pages or a browser runtime platform like Cloudflare Workers, so long as someone somewhere was running a couple of generic services. I wanted to be able to distribute the fullstack server version as basically just an executable in a directory of files with no other dependencies, which could easily be run in any container hosting environment with zero persistent storage required. Hell, you could technically serve it as a blob or series of blobs from your PDS with the right entry point if I did my job right.

I succeeded. 

Well, I don't know if you can serve `weaver-app` purely via  `com.atproto.sync.getBlob` request, but it doesn't need much.
## Constellation
![[at://did:plc:ttdrpj45ibqunmfhdsb4zdwq/app.bsky.feed.post/3m6pckslkt222]] Ana's leaflet does a good job of explaining more or less how Weaver worked up until now. It used direct requests to personal data servers (mostly mine) as well as many calls to [Constellation](https://constellation.microcosm.blue/) and [Slingshot](https://slingshot.microcosm.blue/), and some even to [UFOs](https://ufos.microcosm.blue/), plus a couple of judicious calls to the Bluesky AppView for profiles and post embeds. ![[at://did:plc:hdhoaan3xa3jiuq4fg4mefid/app.bsky.feed.post/3m5jzclsvpc2c]]
The three things linked above are generic services that provide back-links, a record cache, and a running feed of the most recent instances of all lexicons on the network, respectively. That's more than enough to build an app with, though it's not always easy. For some things it can be pretty straightforward. Constellation can tell you what notebooks an entry is in. It can tell you which edit history records are related to this notebook entry. For single-layer relationships it's straightforward. However you then have to also fetch the records individually, because it doesn't provide you the records, just the URIs you need to find them. Slingshot doesn't currently have an endpoint that will batch fetch a list of URIs for you. And the PDS only has endpoints like [`com.atproto.repo.listRecords`](https://docs.bsky.app/docs/api/com-atproto-repo-list-records), which gives you a paginated list of all records of a specific type, but doesn't let you narrow that down easily, so you have to page through until you find what you wanted. 

This wouldn't be too bad if I was fine with almost everything after the hostname in my web URLs being gobbledegook record keys, but I wanted people to be able to link within a notebook like they normally would if they were linking within an Obsidian Vault, by name or by path, something human-readable. So some queries became the good old N+1 requests, because I had to list a lot of records and fetch them until I could find the one that matched. Or worse still, particularly once I introduce collaboration and draft syncing to the editor. Loading a draft of an entry with a lot of edit history could take 100 or more requests, to check permissions, find all the edit records, figure out which ones mattered, publish the collaboration session record, check for collaborators, and so on. It was pretty slow going, particularly when one could not pre-fetch and cache and generate everything server-side on a real CPU rather than in a browser after downloading a nice chunk of WebAssembly code. My profile page [alpha.weaver.sh/nonbinary.computer](https://alpha.weaver.sh/nonbinary.computer) often took quite some time to load due to a frustrating quirk of Dioxus, the Rust web framework I've used for the front-end, which prevented server-side rendering from waiting until everything important had been fetched to render the complete page on that specific route, forcing me to load it client-side.

Some stuff is just complicated to graph out, to find and pull all the relevant data together in order, and some connections aren't the kinds of things you can graph generically. For example, in order to work without any sort of service that has access to indefinite authenticated sessions of more than one person at once, Weaver handles collaborative writing and publishing by having each collaborator write to their own repository and publish there, and then, when the published version is requested, figuring out which version of an entry or notebook is most up-to-date, and displaying that one. It matches by record key across more than one repository, determined at request time by the state of multiple other records in those users' repositories.

# Shape of Data
All of that being said, this was still the correct route, particularly for me. Because not only does this provide a powerful fallback mode, built-in protection against me going AWOL, it was critical in the design process of the index. My friend Ollie, when talking about database and API design, always says that, regardless of the specific technology you use, you need to structure your data based on how you need to query into it. Whatever interface you put in front of it, be it GraphQL, SQL, gRPC, XRPC, server functions, AJAX, literally any way that you can have the part of your app that people interact with pull the specific data they want from where it's stored, how well that performs, how many cycles your server or client spends collecting it, sorting it, or waiting on it, how much memory it takes, how much bandwidth it takes, depends on how that data is shaped, and you, when you are designing your app and all the services that go into it, get to choose that shape. 

Bluesky developers have said that hydrating blocks, mutes, and labels and applying the appropriate ones to the feed content based on the preferences of the user takes quite a bit of compute at scale, and that even the seemingly simple [Following feed](https://jazco.dev/2025/02/19/imperfection/), which is mostly a reverse-chronological feed of posts by people you follow explicitly (plus a few simple rules), is remarkably resource-intensive to produce for them. The extremely clever [string interning](https://jazco.dev/2025/09/26/interning/) and [bitmap tricks](https://jazco.dev/2024/04/20/roaring-bitmaps/) implemented by a brilliant engineer during their time at Bluesky are all oriented toward figuring out the most efficient way to structure the data to make the desired query emerge naturally from it. ![Roaring Bitmaps Diagram from the Original Publication at https://arxiv.org/pdf/1709.07821](https://jazco.dev/public/images/2025-09-26/roaring_bitmaps_diagram.png)

It's intuitive that this matters a lot when you use something like RocksDB, or FoundationDB, or Redis, which are fundamentally key-value stores. What your key contains there determines almost everything about how easy it is to find and manipulate the values you want. Fig and I have had some struggles getting a backup of their Constellation service running in real-time and keeping up with Jetstream on my home server, because the only storage on said home server with enough free space for Constellation's full index is a ZFS pool that's primarily hard-drive based, and the way the Constellation RocksDB backend storage is structured makes processing delete events extremely expensive on a hard drive where seek times are nontrivial. On a Pi 4 with an SSD, it runs just fine. ![[at://did:plc:44ybard66vv44zksje25o7dz/app.bsky.feed.post/3m7e3hnyh5c2u]]
But it's a problem for every database. Custom feed builder service [graze.social](https://graze.social/) ran into difficulties with Postgres early on in their development, as they rapidly gained popularity. They ended up using the same database I did, Clickhouse, for many of the same reasons. ![[at://did:plc:i6y3jdklpvkjvynvsrnqfdoq/app.bsky.feed.post/3m7ecmqcwys23]]
And while thankfully I don't think that a platform oriented around long-form written content will ever have the kinds of following timeline graph write amplification problems Bluesky has dealt with, even if it becomes successful beyond my wildest dreams, there are definitely going to be areas where latency matters a ton and the workload is very write-heavy, like real-time collaboration, particularly if a large number of people work on a document simultaneously, even while the vast majority of requests will primarily be reading data out.

One reason why the edit records for Weaver have three link fields (and may get more!), even though it may seem a bit redundant, is precisely because those links make it easy to graph the relationships between them, to trace a tree of edits backward to the root, while also allowing direct access and a direct relationship to the root snapshot and the thing it's associated with.

In contrast, notebook entry records lack links to other parts of the notebook in and of themselves because calculating them would be challenging, and updating one entry would require not just updating the entry itself and notebook it's in, but also neighbouring entries in said notebook. With the shape of collaborative publishing in Weaver, that would result in up to 4 writes to the PDS when you publish an entry, in addition to any blob uploads. And trying to link the other way in edit history (root to edit head) is similarly challenging.

I anticipated some of these. but others emerged only because I ran into them while building the web app. I've had to manually fix up records more than once because I made breaking changes to my lexicons after discovering I really wanted X piece of metadata or cross-linkage. If I'd built the index first or alongside—particularly if the index remained a separate service from the web app as I intended it to, to keep the web app simple—it would likely have constrained my choices and potentially cut off certain solutions, due to the time it takes to dump the database and re-run backfill even at a very small scale. Building a big chunk of the front end first told me exactly what the index needed to provide easy access to.

You can access it here: [index.weaver.sh](https://index.weaver.sh)
# ClickHAUS
So what does Weaver's index look like? Well it starts with either the firehose or the new [Tap](https://docs.bsky.app/blog/introducing-tap) sync tool. The index ingests from either over a WebSocket connection, does a bit of processing (less is required when ingesting from Tap, and that's currently what I've deployed) and then dumps them in the Clickhouse database. I chose it as the primary index database on recommendation from a friend, and after doing a lot of reading. It fits atproto data well, as Graze found. Because it isolates concurrent inserts and selects so that you can just dump data in, while it cleans things up asynchronously after, it does wonderfully when you have a single major input point or a set of them to dump into that fans out, which you can then transform and then read from. 

I will not claim that the tables you can find in the weaver repository are especially **good** database design overall, but they work, they're very much a work in progress, and we'll see how they scale. Also, Tap makes re-backfilling the data a hell of a lot easier.

This is one of three main input tables. One for record writes, one for identity events, and one for account events.
```SQL
CREATE TABLE IF NOT EXISTS raw_records (
    did String,
    collection LowCardinality(String),
    rkey String,
    cid String,
    -- Repository revision (TID)
    rev String,
    record JSON,
    -- Operation: 'create', 'update', 'delete', 'cache' (fetched on-demand)
    operation LowCardinality(String),
    -- Firehose sequence number
    seq UInt64,
    -- Event timestamp from firehose
    event_time DateTime64(3),
    -- When the database indexed this record
    indexed_at DateTime64(3) DEFAULT now64(3),
    -- Validation state: 'unchecked', 'valid', 'invalid_rev', 'invalid_gap', 'invalid_account'
    validation_state LowCardinality(String) DEFAULT 'unchecked',
    -- Whether this came from live firehose (true) or backfill (false)
    is_live Bool DEFAULT true,
    -- Materialized AT URI for convenience
    uri String MATERIALIZED concat('at://', did, '/', collection, '/', rkey),
    -- Projection for fast delete lookups by (did, cid)
    PROJECTION by_did_cid (
        SELECT * ORDER BY (did, cid)
    )
)
ENGINE = MergeTree()
ORDER BY (collection, did, rkey, event_time, indexed_at);
```
From here we fan out into a cascading series of materialized views and other specialised tables. These break out the different record types, calculate metadata, and pull critical fields out of the record JSON for easier querying. Clickhouse's wild-ass compression means we're not too badly off replicating data on disk this way. Seriously, their JSON type ends up being the same size as a CBOR BLOB on disk in my testing, though it *does* have some quirks, as I discovered when I read back Datetime fields and got...not the format I put in. Thankfully there's a config setting for that. ![Clickhouse animation showing parallel inserts into a source table and a transformation query into a materialized view](https://clickhouse.com/docs/assets/images/incremental_materialized_view-1158726e31b08dc9808d96671239467f.gif)We also build out the list of who contributed to a published entry and determine the canonical record for it, so that fetching a fully hydrated entry with all contributor profiles only takes a couple of `SELECT` queries that themselves avoid performing extensive table scans due to reasonable choices of `ORDER BY` fields in the denormalized tables they query and are thus very fast. And then I can do quirky things like power a profile fetch endpoint that will provide either a Weaver or a Bluesky profile, while also unifying fields so that we can easily get at the critical stuff in common. This is a relatively expensive calculation, but people thankfully don't edit their profiles that often, and this is why we don't keep the stats in the same table. 

However, this is ***also*** why Clickhouse will not be the only database used in the index.

# Why is it always SQLite?
When it comes to things like real-time collaboration sessions with almost keystroke-level cursor tracking and rapid per-user writeback/readback, where latency matters and we can't wait around for the merge cycle to produce the right state, *don't* work well in Clickhouse. But they sure do in SQLite!

If there's one thing the AT Protocol developer community loves more than base32-encoded timestamps it's SQLite. In fairness, we're in good company, the whole world loves SQLite. It's a good fucking embedded database and very hard to beat for write or read performance so long as you're not trying to hit it massively concurrently. Of course, that concurrency limitation does end up mattering as you scale. And here we take a cue from the Typescript PDS implementation and discover the magic of buying, well, a lot more than two of them, and of using the filesystem like a hierarchical key-value store.

<iframe width="560" height="315" src="https://www.youtube.com/embed/CZs-YcmxyUw?si=bd3GmSxMVQGdqHAR" title="YouTube video player" frameborder="0" allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share" referrerpolicy="strict-origin-when-cross-origin" allowfullscreen></iframe>

This part of the data backend is still *very* much a work-in-progress and isn't used yet in the deployed version, but I did want to discuss the architecture. Unlike the PDS, we don't divide primarily by DID, instead we shard by resource, designated by collection and record key.

```rust
pub struct ShardKey {
    pub collection: SmolStr,
    pub rkey: SmolStr,
}

impl ShardKey {
...
    /// Directory path: {base}/{hash(collection,rkey)[0..2]}/{rkey}/
    fn dir_path(&self, base: &Path) -> PathBuf {
        base.join(self.hash_prefix()).join(self.rkey.as_str())
    }
...
}
/// A single SQLite shard for a resource
pub struct SqliteShard {
    conn: Mutex<Connection>,
    path: PathBuf,
    last_accessed: Mutex<Instant>,
}
/// Routes resources to their SQLite shards
pub struct ShardRouter {
    base_path: PathBuf,
    shards: DashMap<ShardKey, std::sync::Arc<SqliteShard>>,
}
```

The hash of the shard key plus the record key gives us the directory where we put the database file for this resource. Ultimately this may be moved out of the main index off onto something more comparable to the Tangled knot server or Streamplace nodes, depending on what constraints we run into if things go exceptionally well, but for now it lives as part of the index. In there we can tee off raw events from the incoming firehose and then transform them into the correct forms in memory, optionally persisted to disk, alongside Clickhouse and probably, for the specific things we want it for with a local scope, faster. 

And direct communication, either by using something like oatproxy to swap the auth relationships around a bit (currently the index is accessed via service proxying through the PDS when authenticated) or via an iroh channel from the client, gets stuff there without having to wait for the relay to pick it up and fan it out to us, which then means that users can read their own writes very effectively. The handler hits the relevant SQLite shard if present and Clickhouse in parallel, merging the data to provide the most up-to-date form. For real-time collaboration this is critical. The current `iroh-gossip` implementation works well and requires only a generic iroh relay, but it runs into the problem every gossip protocol runs into the more concurrent users you have.

The exact method of authentication of that side-channel is by far the largest remaining unanswered question about Weaver right now, aside from "Will anyone (else) use it?" 

If people have ideas, I'm all ears.

## Future
Having this available obviously improves the performance of the app, but it also enables a lot of new stuff. I have plans for social features which would have been much harder to implement without it, and can later be backfilled into the non-indexed implementation. I have more substantial rewrites of the data fetching code planned as well, beyond the straightforward replacement I did in this first pass. And there's still a **lot** more to do on the editor before it's done.

I've been joking about all sorts of ambitious things, but legitimately I think Weaver ends up being almost uniquely flexible and powerful among the atproto-based long-form writing platforms with how it's designed, and in particular how it enables people to create things together, and can end up filling some big shoes, given enough time and development effort.

I hope you found this interesting. I enjoyed writing it out. There's still a lot more to do, but this was a big milestone for me. 

If you'd like to support this project, there's a GitHub Sponsorship link at the bottom of the page, but honestly I'd love if you used it to write something.