# Website

`index.html` and `download.html` are the templates; `site/locales/*.json` supplies all application languages.
The download page (`/download/` and `/<locale>/download/`) reads the latest release from the GitHub API
in the browser and links every package to its file there, so it needs no change per release; without the
API its links open the latest release page.
Run `sh scripts/assemble-site.sh` after the web build, then
`python3 scripts/generate-star-history.py --output-dir dist` for the charts.
Run `python3 scripts/test_site.py` to check translations, links, metadata and language selection.

The root page follows the browser language, falling back to English. Language links
use explicit locale paths and remember manual choices when browser storage is available.
Arabic uses a right-to-left layout. The favicon uses the centered OCS lettering
from `assets/logo.svg`, without details that disappear at small sizes. Export
`favicon.svg` to PNG at 96, 180, 192 and 512 pixels and ICO at 16, 32, 48, 96 and
256 pixels when it changes. Favicon URLs stay fixed for search engine crawlers.

`workspace.png` and `modeling.png` are shared by the website and all README languages.
The drawing screenshot also supplies the social preview; website image URLs change with their contents.

GitHub Pages runs on release publication, a call from the weekly release workflow,
or a manual run. Commits and pushes do not start workflows. Deployments use website
files from `main` and build the app from the release, recording both sources separately.
Manual runs can select **build_main** to publish a web hotfix; see `docs/releases.md`.
