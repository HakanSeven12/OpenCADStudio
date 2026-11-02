function preferredLocale(language) {
  const tag = (language || "").toLowerCase();
  const exact = SITE_LOCALES.find(locale => locale.toLowerCase() === tag);
  if (exact) return exact;
  const base = tag.split("-")[0];
  if (base === "zh") {
    return /(?:^|-)hant(?:-|$)|(?:^|-)(tw|hk|mo)(?:-|$)/.test(tag) ? "zh-TW" : "zh-CN";
  }
  return SITE_LOCALES.find(locale => locale.split("-")[0] === base) || "en-US";
}

(() => {
  const storageKey = "ocs.site.language";
  let saved;
  try { saved = localStorage.getItem(storageKey); } catch {}
  const locale = SITE_LOCALES.includes(saved) ? saved : preferredLocale(navigator.language);
  if ((location.pathname === "/" || location.pathname === "/index.html") && locale !== "en-US") {
    location.replace(`/${locale}/${location.search}${location.hash}`);
  } else if ((location.pathname === "/download/" || location.pathname === "/download/index.html") && locale !== "en-US") {
    location.replace(`/${locale}/download/${location.search}${location.hash}`);
  }
  document.addEventListener("click", event => {
    const link = event.target.closest(".language-picker a[hreflang]");
    if (link) {
      try { localStorage.setItem(storageKey, link.hreflang); } catch {}
    } else if (!event.target.closest(".language-picker")) {
      document.querySelector(".language-picker[open]")?.removeAttribute("open");
    }
  });
  document.addEventListener("keydown", event => {
    if (event.key !== "Escape") return;
    const picker = document.querySelector(".language-picker[open]");
    if (picker) {
      picker.removeAttribute("open");
      picker.querySelector("summary").focus();
    }
  });
})();

// Download page: every link points at the latest release's own file, read
// from the GitHub API, so the page needs no change per release. Without the
// API the links keep pointing at the latest release page.
(() => {
  const start = () => {
    const page = document.querySelector("[data-downloads]");
    if (!page?.dataset?.releaseApi) return;
    const rows = [...page.querySelectorAll(".dl-row[data-suffix]")];
    const cacheKey = "ocs.site.latestRelease";

    const loadRelease = async () => {
      try {
        const cached = JSON.parse(sessionStorage.getItem(cacheKey) || "null");
        if (cached && Date.now() - cached.at < 10 * 60 * 1000) return cached.release;
      } catch {}
      const response = await fetch(page.dataset.releaseApi, { headers: { Accept: "application/vnd.github+json" } });
      if (!response.ok) throw new Error(`release lookup ${response.status}`);
      const data = await response.json();
      const release = {
        tag: data.tag_name,
        url: data.html_url,
        published: data.published_at,
        assets: (data.assets || [])
          .filter(asset => asset.size > 0 && asset.state === "uploaded")
          .map(asset => ({ name: asset.name, url: asset.browser_download_url, size: asset.size })),
      };
      try { sessionStorage.setItem(cacheKey, JSON.stringify({ at: Date.now(), release })); } catch {}
      return release;
    };

    // The visitor's system: os is windows, macos, linux or mobile; arch is
    // x64 or arm64 when it can be told.
    const detect = async () => {
      const ua = navigator.userAgent;
      let os = /Android|iPhone|iPad|iPod/i.test(ua) ? "mobile"
        : /Windows/i.test(ua) ? "windows"
        : /Macintosh|Mac OS X/i.test(ua) ? (navigator.maxTouchPoints > 1 ? "mobile" : "macos")
        : /Linux|X11|CrOS/i.test(ua) ? "linux" : "";
      let arch = /aarch64|arm64|armv8/i.test(ua) ? "arm64" : "";
      try {
        const hints = await navigator.userAgentData?.getHighEntropyValues?.(["architecture", "bitness"]);
        if (hints?.architecture === "arm") arch = "arm64";
        else if (hints?.architecture === "x86") arch = "x64";
      } catch {}
      if (os === "macos" && !arch) {
        // Safari reports an Intel Mac on Apple Silicon too; the GPU tells.
        try {
          const gl = document.createElement("canvas").getContext("webgl");
          const info = gl && gl.getExtension("WEBGL_debug_renderer_info");
          const renderer = info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : "";
          arch = /Apple (M\d|GPU)/i.test(renderer) ? "arm64" : /Intel|AMD|Radeon/i.test(renderer) ? "x64" : "arm64";
        } catch { arch = "arm64"; }
      }
      return { os, arch: arch || "x64" };
    };

    const megabytes = bytes => `${(bytes / 1048576).toFixed(bytes < 10485760 ? 1 : 0)} MB`;

    Promise.all([loadRelease(), detect()]).then(([release, system]) => {
      for (const row of rows) {
        const asset = release.assets.find(asset => asset.name.endsWith(`-${row.dataset.suffix}`));
        if (asset) {
          row.href = asset.url;
          row.dataset.file = asset.name;
          row.querySelector("[data-size]").textContent = megabytes(asset.size);
        } else {
          row.classList.add("is-missing");
          row.setAttribute("aria-disabled", "true");
          row.removeAttribute("href");
          const note = document.createElement("small");
          note.className = "dl-missing";
          note.textContent = page.dataset.unavailable;
          row.querySelector(".dl-row-text").append(note);
        }
      }

      const line = page.querySelector("[data-release-line]");
      line.querySelector("[data-version]").textContent = release.tag.replace(/^v/, "");
      const date = new Date(release.published);
      const time = line.querySelector("[data-date]");
      time.dateTime = release.published;
      time.textContent = date.toLocaleDateString(document.documentElement.lang, { year: "numeric", month: "long", day: "numeric" });
      line.querySelector("[data-notes]").href = release.url;
      line.hidden = false;

      if (system.os === "mobile" || !system.os) return;
      const card = page.querySelector(`.dl-card[data-os="${system.os}"]`);
      const options = [...card.querySelectorAll(".dl-row:not(.is-missing)")];
      // The first package for the visitor's processor (installer before
      // portable on Windows), else the first one for the other processor.
      const pick = options.find(row => row.dataset.arch === system.arch) || options[0];
      if (!pick) return;
      card.classList.add("is-current");
      pick.classList.add("is-current");
      const primary = page.querySelector("[data-primary]");
      primary.href = pick.href;
      const platform = card.querySelector("h2").textContent;
      const variant = pick.querySelector("small").textContent.split("·")[0].trim();
      primary.querySelector("[data-primary-label]").textContent =
        page.dataset.downloadFor.replace("{platform}", `${platform} (${variant})`);
      page.querySelector("[data-primary-file]").textContent = `${pick.dataset.file} · ${pick.querySelector("[data-size]").textContent}`;
      page.querySelector("[data-recommended]").hidden = false;
    }).catch(() => {
      // No API: every link already opens the latest release page.
    });
  };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start);
  else start();
})();
