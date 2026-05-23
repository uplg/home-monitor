const LEGACY_CACHE_PREFIXES = ["home-monitor-v"];

function hasLegacyWorkerScript(worker: ServiceWorker | null): worker is ServiceWorker {
  return Boolean(worker && new URL(worker.scriptURL).pathname === "/sw.js");
}

export async function cleanupLegacyPwaArtifacts(): Promise<void> {
  if (typeof window === "undefined") {
    return;
  }

  if ("serviceWorker" in navigator) {
    const registrations = await navigator.serviceWorker.getRegistrations();

    await Promise.all(
      registrations
        .filter((registration) => {
          return [registration.active, registration.waiting, registration.installing].some(
            hasLegacyWorkerScript,
          );
        })
        .map((registration) => registration.unregister()),
    );
  }

  if ("caches" in window) {
    const cacheKeys = await caches.keys();

    await Promise.all(
      cacheKeys
        .filter((cacheKey) => {
          return LEGACY_CACHE_PREFIXES.some((prefix) => cacheKey.startsWith(prefix));
        })
        .map((cacheKey) => caches.delete(cacheKey)),
    );
  }
}
