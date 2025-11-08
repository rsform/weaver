// Weaver Service Worker
// Handles blob/image requests by intercepting fetch and serving from PDS

const CACHE_NAME = "weaver-blobs-v1";

// Map of notebook/path -> real URL
// e.g., "notebook/image/foo.jpg" -> "https://pds.example.com/xrpc/com.atproto.sync.getBlob?..."
const urlMappings = new Map();

// Install and activate immediately
self.addEventListener("install", (event) => {
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(clients.claim());
});

// Receive mappings from main thread
self.addEventListener("message", (event) => {
  if (event.data.type === "register_mappings") {
    const notebook = event.data.notebook;
    // Store blob URL mappings
    for (const [name, url] of Object.entries(event.data.blobs)) {
      const key = `${notebook}/image/${name}`;
      urlMappings.set(key, url);
    }
  }
});

// Intercept fetch requests
self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);

  // Extract key from path (e.g., "/notebook/image/foo.jpg" -> "notebook/image/foo.jpg")
  const pathParts = url.pathname.split("/").filter((p) => p);

  // Check if this looks like an image request (format: /:notebook/image/:name)
  if (pathParts.length >= 3 && pathParts[pathParts.length - 2] === "image") {
    // Reconstruct the key
    const key = pathParts.join("/");

    const mapping = urlMappings.get(key);
    if (mapping) {
      event.respondWith(handleBlobRequest(mapping, key));
      return;
    }
  }

  // Let other requests pass through
});

async function handleBlobRequest(url, key) {
  try {
    // Check cache first
    const cache = await caches.open(CACHE_NAME);
    let response = await cache.match(key);

    if (response) {
      return response;
    }

    // Fetch from PDS
    response = await fetch(url);

    if (!response.ok) {
      console.error("[SW] Fetch failed:", response.status, response.statusText);
      return new Response("Blob not found", { status: 404 });
    }

    // Cache the response
    await cache.put(key, response.clone());

    return response;
  } catch (error) {
    console.error("[SW] Error handling blob request:", error);
    return new Response("Error fetching blob", { status: 500 });
  }
}
