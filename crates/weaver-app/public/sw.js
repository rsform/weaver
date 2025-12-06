// Weaver Service Worker
// Handles blob/image requests by caching immutable images
//
// URL patterns handled:
// - /image/{ident}/draft/{blob_rkey}/{name}  - draft images (unpublished)
// - /image/{ident}/{entry_rkey}/{name}       - published entry images
// - /image/{notebook}/{name}                 - notebook images (legacy)
// - /{notebook}/image/{name}                 - notebook images (legacy path)

const CACHE_NAME = "weaver-blobs-v2";

// Map of notebook/path -> real URL for legacy blob mappings
// e.g., "notebook/image/foo.jpg" -> "https://pds.example.com/xrpc/com.atproto.sync.getBlob?..."
const urlMappings = new Map();

// Install and activate immediately
self.addEventListener("install", (event) => {
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    Promise.all([
      clients.claim(),
      // Clean up old cache versions
      caches
        .keys()
        .then((names) =>
          Promise.all(
            names
              .filter((name) => name.startsWith("weaver-blobs-") && name !== CACHE_NAME)
              .map((name) => caches.delete(name)),
          ),
        ),
    ]),
  );
});

// Receive mappings from main thread (for legacy notebook images)
self.addEventListener("message", (event) => {
  if (event.data.type === "register_mappings") {
    const notebook = event.data.notebook;
    // Store blob URL mappings
    for (const [name, url] of Object.entries(event.data.blobs)) {
      const key = `${notebook}/image/${name}`;
      urlMappings.set(key, url);
    }
  }
  if (event.data.type === "register_rkey_mappings") {
    const rkey = event.data.rkey;
    const ident = event.data.ident;
    // Store blob URL mappings
    for (const [name, url] of Object.entries(event.data.blobs)) {
      const key = `/image/${ident}/${rkey}/${name}`;
      urlMappings.set(key, url);
    }
  }
});

// Check if a path is an image request we should cache
function isImagePath(pathname) {
  // New format: /image/{ident}/{...}/{name}
  if (pathname.startsWith("/image/")) {
    return true;
  }
  // Legacy format: /{notebook}/image/{name}
  const parts = pathname.split("/").filter((p) => p);
  if (parts.length >= 3 && parts[parts.length - 2] === "image") {
    return true;
  }
  return false;
}

// Intercept fetch requests
self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);

  // Only handle same-origin requests
  if (url.origin !== self.location.origin) {
    return;
  }

  // Check if this is an image request
  if (!isImagePath(url.pathname)) {
    return;
  }

  // Use pathname as cache key
  const cacheKey = url.pathname;

  // Extract path parts for legacy mapping lookup
  const pathParts = url.pathname.split("/").filter((p) => p);

  // Check legacy mappings (for /{notebook}/image/{name} format)
  if (pathParts.length >= 3 && pathParts[pathParts.length - 2] === "image") {
    const legacyKey = pathParts.join("/");
    const mapping = urlMappings.get(legacyKey);
    if (mapping) {
      event.respondWith(handleBlobRequest(mapping, cacheKey));
      return;
    }
  }
  if (pathParts.length >= 4 && pathParts[0] === "image") {
    const legacyKey = pathParts.join("/");
    const mapping = urlMappings.get(legacyKey);
    if (mapping) {
      event.respondWith(handleBlobRequest(mapping, cacheKey));
      return;
    }
  }

  // For new /image/... routes, cache the response from our server
  event.respondWith(handleImageRequest(event.request, cacheKey));
});

// Handle requests that have a direct URL mapping (legacy)
async function handleBlobRequest(url, cacheKey) {
  try {
    const cache = await caches.open(CACHE_NAME);
    let response = await cache.match(cacheKey);

    if (response) {
      return response;
    }

    // Fetch from PDS
    response = await fetch(url);

    if (!response.ok) {
      console.error("[SW] Fetch failed:", response.status, response.statusText);
      return new Response("Blob not found", { status: 404 });
    }

    // Cache the response (blobs are immutable by CID)
    await cache.put(cacheKey, response.clone());

    return response;
  } catch (error) {
    console.error("[SW] Error handling blob request:", error);
    return new Response("Error fetching blob", { status: 500 });
  }
}

// Handle image requests via our server (new /image/... routes)
async function handleImageRequest(request, cacheKey) {
  try {
    const cache = await caches.open(CACHE_NAME);
    let response = await cache.match(cacheKey);

    if (response) {
      return response;
    }

    // Fetch from our server
    response = await fetch(request);

    if (!response.ok) {
      // Don't cache error responses
      return response;
    }

    // Check if response is cacheable (has immutable cache-control)
    const cacheControl = response.headers.get("cache-control") || "";
    if (cacheControl.includes("immutable") || cacheControl.includes("max-age=31536000")) {
      // Cache the response
      await cache.put(cacheKey, response.clone());
    }

    return response;
  } catch (error) {
    console.error("[SW] Error handling image request:", error);
    return new Response("Error fetching image", { status: 500 });
  }
}
