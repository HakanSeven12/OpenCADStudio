# Releases

The **Weekly release** workflow runs every Sunday at 12:00 UTC. GitHub may
queue the run after that time. Weeks without new commits are skipped.

The release name uses the UTC ISO week year and week, such as `2026.35`.
The workflow updates `Cargo.toml` and `Cargo.lock`, commits `Release v2026.35`
on `main`, and atomically pushes that commit and its `v2026.35` tag.
It creates release notes using the existing section format and verifies the
published title and notes.

Web and native workflows receive the same tag and commit and run independently.
Web deploys when its own build finishes; native packages are attached as they
finish. The web deployment verifies `/app/release.json`, and the native
workflow verifies all five download files. Native update notices require a
download for the installed platform.

The app shows `2026.35`, Cargo and macOS use `2026.35.0`, and MSI uses `26.35.0`.
The main window title is `Open CAD Studio 2026.35 - Drawing.dwg`.

To preview release notes, manually run **Weekly release** on `main` with
**publish** unchecked. Check **publish** to release immediately. Rerunning
within the same week reuses the original tag and commit. To rebuild just a
failed target, use GitHub's **Re-run failed jobs**. Manual web deployment
always uses the latest published release rather than unreleased `main`.
