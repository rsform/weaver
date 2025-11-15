## Viewer/minimal app

#### Routes - names can be title/name or rkey, ident can be did or handle
/{ident}/{notebook}/{entry} - entry
/{ident}/{notebook}/{page} - page of entries
/{ident}/{notebook}/{chapter}/{entry|page} - entry/page in chapter
/{ident}/{notebook} - index/"cover page"
/{ident} - profile page

/ - discover (later, home)


/record/{at-uri} - fallback viewer?
/post/{at-uri} - bsky post, anything similarly shaped
/leaflet/{pub}/{rkey} - leaflet
/blog/{at-uri} - whitewind, etc.
/feed/{at-uri}


/{notebook}/image/{name|cid}
/{notebook}/blob/{cid}
/{ident}/{notebook}/css