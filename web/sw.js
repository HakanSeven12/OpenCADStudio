// Open CAD Studio web app service worker: offline app-shell caching.
//
// Trunk copies this file to dist/app/sw.js (copy-file link in web-app.html),
// so it is served at /app/sw.js and its scope is /app/ — the whole app. The
// page registers it with a relative URL; see the inline script in
// web-app.html.
//
// Caching strategy
//   - Navigations (any path under /app/, deep links included): network-first.
//     A successful response is cached under the canonical shell URL (the
//     scope root) and returned, so updates land on the next visit; offline
//     the cached shell is served instead, which also serves as the fallback
//     for deep-linked paths.
//   - Other same-origin GET requests (wasm, js, workers, fonts, JSON):
//     cache-first with a background revalidate. Trunk content-hashes the
//     heavy assets (OpenCADStudio-<hash>[_bg].{js,wasm}), so a cached copy
//     can never be stale and revalidation is effectively free; non-hashed
//     files (fonts/*.ttf, *.json, ocs-*.js) are silently refreshed in the
//     background. Everything the app loads is captured during the first
//     online visit, after which the ~55 MB bundle is served from the cache
//     instead of re-downloaded on every visit.
//   - Non-GET and cross-origin requests: passthrough (never intercepted).
//
// Versioning
//   CACHE_VERSION below is a placeholder ("v1") so this file works verbatim
//   under `trunk serve`. scripts/build-web.sh stamps it with a hash of the
//   finished bundle after every build, so each deploy ships a fresh cache
//   name and the activate step below deletes the previous deploy's cache.
//
// Update flow
//   A changed sw.js installs a new worker, but this code deliberately never
//   calls skipWaiting() and never reloads the page: the browser keeps the
//   new worker in the "waiting" state until every tab running the old
//   version has closed, so an in-progress editing session is never disrupted
//   mid-session. The new worker (and its new cache) takes over on the first
//   visit after that. Nothing is shown to the user — the browser chrome
//   already handles the "update available" UX.

const CACHE_VERSION = "v1"; // stamped by scripts/build-web.sh (bundle hash)
const CACHE_NAME = `ocs-app-${CACHE_VERSION}`;
const SHELL_URL = new URL("./", self.registration.scope);

self.addEventListener("install", (event) => {
  // Precache the app shell (index.html) so offline navigation works from the
  // first visit after registration. If the shell cannot be fetched the
  // install fails and the browser retries on a later navigation — a broken
  // shell must never be cached.
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) => cache.addAll([SHELL_URL.href])),
  );
});

self.addEventListener("activate", (event) => {
  // Delete every cache from previous versions, then start controlling pages
  // that loaded before this worker was ready, so the current visit's requests
  // are captured without needing a reload.
  event.waitUntil(
    (async () => {
      const names = await caches.keys();
      await Promise.all(
        names
          .filter((name) => name !== CACHE_NAME)
          .map((name) => caches.delete(name)),
      );
      await self.clients.claim();
    })(),
  );
});

// Cache a copy of a response unless it is not a complete success. Quota or
// write errors are swallowed: caching must never break the app itself.
const put = async (cache, key, response) => {
  if (!response || !response.ok) return;
  try {
    await cache.put(key, response.clone());
  } catch {}
};

self.addEventListener("fetch", (event) => {
  const request = event.request;

  // Only same-origin GETs are ours to serve.
  if (request.method !== "GET") return;
  if (new URL(request.url).origin !== self.location.origin) return;
  // Range requests (media seeking) cannot be served from a full-body cache.
  if (request.headers.has("range")) return;

  if (request.mode === "navigate") {
    event.respondWith(
      (async () => {
        const cache = await caches.open(CACHE_NAME);
        // Only the scope root is the app shell: caching any other in-scope
        // navigation here would store e.g. a font or JSON body under the
        // shell key and serve it for every offline navigation.
        const isShell = new URL(request.url).pathname === SHELL_URL.pathname;
        try {
          const fresh = await fetch(request);
          if (isShell) await put(cache, SHELL_URL, fresh);
          return fresh;
        } catch {
          const cached = await cache.match(SHELL_URL);
          if (cached) return cached;
          return new Response("Open CAD Studio is offline.", {
            status: 503,
            headers: {"Content-Type": "text/plain; charset=utf-8"},
          });
        }
      })(),
    );
    return;
  }

  // Cache-first with background revalidate. On a hit the cached copy is
  // returned immediately and a fresh one is fetched (and cached) in the
  // background; on a miss the network response is cached before being
  // returned, making the next visit offline-capable.
  event.respondWith(
    (async () => {
      const cache = await caches.open(CACHE_NAME);
      const cached = await cache.match(request);
      const network = fetch(request)
        .then(async (fresh) => {
          await put(cache, request, fresh);
          return fresh;
        })
        .catch(() => undefined);
      if (cached) {
        // Keep the worker alive until the background revalidate finishes.
        event.waitUntil(network);
        return cached;
      }
      const fresh = await network;
      if (fresh) return fresh;
      return new Response("Open CAD Studio is offline.", {
        status: 503,
        headers: {"Content-Type": "text/plain; charset=utf-8"},
      });
    })(),
  );
});
