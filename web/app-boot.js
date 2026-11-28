// Offline support: register the service worker. It is copied next to
// web-app.html by Trunk, so the relative URL resolves to /app/sw.js and
// the registration scope is /app/ — the whole app.
(() => {
  if (self.origin === "null" || !("serviceWorker" in navigator) || new URLSearchParams(location.search).get("embed") === "1") return;
  try {
    navigator.serviceWorker.register("sw.js").catch(error => {
      console.warn("service worker registration failed; offline support disabled:", error);
    });
  } catch (error) {
    console.warn("service worker unavailable; offline support disabled:", error);
  }
})();

(() => {
  const splash = document.getElementById("loading");
  fetch("locale-labels.json").then(response => response.json()).then(labels => {
    let saved;
    try {
      saved = JSON.parse(localStorage.getItem("opencadstudio.settings"))?.settings?.language;
    } catch {}
    const preferred = navigator.languages?.length ? navigator.languages : [navigator.language];
    const requested = typeof saved === "string" && saved !== "system" ? [saved, ...preferred] : preferred;
    const available = Object.keys(labels);
    let locale = "en-US";
    for (const candidate of requested) {
      try {
        const language = new Intl.Locale(candidate);
        const match = available.find(value => value.toLowerCase() === language.baseName.toLowerCase())
          || (language.language === "zh"
            ? (language.maximize().script === "Hant" ? "zh-TW" : "zh-CN")
            : available.find(value => value.startsWith(language.language + "-")));
        if (match) { locale = match; break; }
      } catch {}
    }
    document.title = labels[locale].title;
    document.documentElement.lang = locale;
    document.documentElement.dir = locale === "ar-SA" ? "rtl" : "ltr";
    const caption = splash?.querySelector(".sub");
    if (caption) caption.textContent = labels[locale].loading;
  }).catch(() => {});
  const done = () => {
    if (!splash) return;
    splash.classList.add("hide");
    setTimeout(() => splash.remove(), 450);
  };
  if (document.querySelector("canvas")) return done();
  const observer = new MutationObserver(() => {
    if (document.querySelector("canvas")) {
      observer.disconnect();
      done();
    }
  });
  observer.observe(document.body, { childList: true, subtree: true });
})();
