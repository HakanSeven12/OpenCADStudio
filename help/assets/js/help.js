(function () {
    "use strict";

    const body = document.body;
    const toggle = document.querySelector(".nav-toggle");
    const sidebar = document.getElementById("help-sidebar");
    const search = document.getElementById("help-search");
    const results = document.getElementById("search-results");
    const mobile = window.matchMedia("(max-width: 760px)");
    let selected = -1;

    function syncToggle() {
        const expanded = mobile.matches
            ? body.classList.contains("nav-open")
            : !body.classList.contains("nav-collapsed");
        toggle?.setAttribute("aria-expanded", String(expanded));
    }

    toggle?.addEventListener("click", function () {
        body.classList.toggle(mobile.matches ? "nav-open" : "nav-collapsed");
        syncToggle();
    });

    sidebar?.addEventListener("click", function (event) {
        if (mobile.matches && event.target.closest("a")) {
            body.classList.remove("nav-open");
            syncToggle();
        }
    });

    mobile.addEventListener("change", function () {
        body.classList.remove("nav-open");
        syncToggle();
    });

    function hideResults() {
        results.hidden = true;
        results.innerHTML = "";
        selected = -1;
        search?.setAttribute("aria-expanded", "false");
    }

    function renderResults(query) {
        const terms = query.toLocaleLowerCase().trim().split(/\s+/).filter(Boolean);
        if (!terms.length) {
            hideResults();
            return;
        }

        const matches = (window.OCS_HELP_INDEX || []).map(function (entry) {
            const title = entry.title.toLocaleLowerCase();
            const text = entry.text.toLocaleLowerCase();
            const score = terms.reduce(function (total, term) {
                if (!text.includes(term) && !title.includes(term)) return -1000;
                return total + (title.includes(term) ? 8 : 1);
            }, 0);
            return { entry: entry, score: score };
        }).filter(function (item) { return item.score >= 0; })
          .sort(function (a, b) { return b.score - a.score || a.entry.title.localeCompare(b.entry.title); })
          .slice(0, 12);

        results.innerHTML = "";
        if (!matches.length) {
            const empty = document.createElement("li");
            empty.className = "search-empty";
            empty.textContent = "No matching help topics.";
            results.appendChild(empty);
        } else {
            matches.forEach(function (item) {
                const li = document.createElement("li");
                const link = document.createElement("a");
                link.href = item.entry.url;
                const strong = document.createElement("strong");
                strong.textContent = item.entry.title;
                const detail = document.createElement("span");
                detail.textContent = item.entry.chapter;
                link.append(strong, detail);
                li.appendChild(link);
                results.appendChild(li);
            });
        }
        results.hidden = false;
        search.setAttribute("aria-expanded", "true");
        selected = -1;
    }

    search?.addEventListener("input", function () { renderResults(search.value); });
    search?.addEventListener("keydown", function (event) {
        const links = Array.from(results.querySelectorAll("a"));
        if (event.key === "Escape") {
            search.value = "";
            hideResults();
            return;
        }
        if (!links.length || !["ArrowDown", "ArrowUp", "Enter"].includes(event.key)) return;
        event.preventDefault();
        if (event.key === "Enter" && selected >= 0) {
            links[selected].click();
            return;
        }
        if (event.key === "ArrowDown") selected = (selected + 1) % links.length;
        if (event.key === "ArrowUp") selected = selected < 0 ? links.length - 1 : (selected - 1 + links.length) % links.length;
        links.forEach(function (link, index) { link.setAttribute("aria-selected", String(index === selected)); });
        links[selected]?.scrollIntoView({ block: "nearest" });
    });

    document.addEventListener("keydown", function (event) {
        const shortcut = event.key === "/" || ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k");
        if (shortcut && document.activeElement !== search) {
            event.preventDefault();
            search?.focus();
        }
    });

    document.addEventListener("click", function (event) {
        if (!event.target.closest(".search-shell")) hideResults();
    });

    syncToggle();
}());
