// Weaver Service Worker
// Handles blob/image requests by intercepting fetch and serving from PDS

const CACHE_NAME = "weaver-blobs-v1";

// Map of notebook/path -> real URL
// e.g., "notebook/image/foo.jpg" -> "https://pds.example.com/xrpc/com.atproto.sync.getBlob?..."
const urlMappings = new Map();

// Install and activate immediately
self.addEventListener("install", (event) => {
  console.log("[SW] Installing service worker");
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  console.log("[SW] Activating service worker");
  event.waitUntil(clients.claim());
});

// Receive mappings from main thread
self.addEventListener("message", (event) => {
  if (event.data.type === "register_mappings") {
    const notebook = event.data.notebook;
    console.log("[SW] Registering blob mappings for notebook:", notebook);

    // Store blob URL mappings
    for (const [name, url] of Object.entries(event.data.blobs)) {
      const key = `${notebook}/image/${name}`;
      urlMappings.set(key, url);
      console.log("[SW] Registered mapping:", key, "->", url);
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

    console.log("[SW] Intercepted image request:", key);

    const mapping = urlMappings.get(key);
    if (mapping) {
      console.log("[SW] Found mapping for:", key, "->", mapping);
      event.respondWith(handleBlobRequest(mapping, key));
      return;
    } else {
      console.log("[SW] No mapping found for:", key);
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
      console.log("[SW] Cache hit for:", key);
      return response;
    }

    // Fetch from PDS
    console.log("[SW] Fetching from PDS:", url);
    response = await fetch(url);

    if (!response.ok) {
      console.error("[SW] Fetch failed:", response.status, response.statusText);
      return new Response("Blob not found", { status: 404 });
    }

    // Cache the response
    await cache.put(key, response.clone());
    console.log("[SW] Cached blob:", key);

    return response;
  } catch (error) {
    console.error("[SW] Error handling blob request:", error);
    return new Response("Error fetching blob", { status: 500 });
  }
}
