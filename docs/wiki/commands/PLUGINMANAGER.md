---
summary: Opens the Plugin Manager, where you install, enable, disable, update and remove plugins.
related: PLUGINS, OPTIONS
web: Not available in the web app; it shows a notice with a link to download the desktop app.
---

## Usage
1. Start `PLUGINMANAGER`. The Plugin Manager opens.
2. Use the search box to find a plugin.
3. Select a plugin to read its description.
4. Use the buttons on a plugin card to act on it.

## Options
| Action | What it does |
|---|---|
| **Install** | Installs a plugin from the list of available plugins. |
| **Enable** / **Disable** | Turns an installed plugin on or off. A disabled plugin stays listed, but its ribbon tab and commands are removed. The choice is kept between sessions. |
| **Update to** | Installs a newer release of the plugin. |
| **Uninstall** | Removes the plugin. |
| **Add from GitHub** | Adds a plugin from a GitHub repository. Type its URL or `owner/repository`. |

## Notes
- The list of available plugins is loaded from the online plugin catalog. If it cannot be loaded, an error with **Retry** and **Show details** is shown.
- Plugins bundled with the program are marked **Bundled with Open CAD Studio**.
- A plugin built for an incompatible plugin API is marked **API incompatible**.
- Some changes take effect after a restart; the card then shows **Restart to load**.
- You can also open the Plugin Manager from the Options dialog.
