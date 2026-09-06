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
  }
  document.addEventListener("click", event => {
    const link = event.target.closest(".language-picker a[hreflang]");
    if (!link) return;
    try { localStorage.setItem(storageKey, link.hreflang); } catch {}
  });
})();
