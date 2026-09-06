# Website

`index.html` is the template; `site/locales/*.json` supplies all 21 application languages.
Run `sh scripts/assemble-site.sh` after the web build, then
`python3 scripts/generate-star-history.py --output-dir dist` for the charts.
Run `python3 scripts/test_site.py` to check translations, links, metadata and language selection.

The root page follows the browser language, falling back to English. Language links
use explicit locale paths and remember manual choices when browser storage is available.
Arabic uses a right-to-left layout. Icons are exported from `assets/logo.svg`;
update the PNG/ICO files in this directory when the logo changes.

`workspace.png` and `modeling.png` are shared by the website and all README languages.
The drawing screenshot also supplies the social preview; website image URLs change with their contents.

GitHub Pages publishes website changes from `main` while building the app from the
latest release. Each deployment records both sources separately.
