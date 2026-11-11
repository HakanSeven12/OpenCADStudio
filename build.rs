// Build metadata and Windows executable resources.

use std::path::{Path, PathBuf};

fn main() {
    let version = std::env::var("CARGO_PKG_VERSION").expect("Cargo package version");
    let parts: Vec<&str> = version.split('.').collect();
    let app_version = if parts.len() == 3 && parts[0].len() == 4 && parts[0].starts_with("20") {
        let week = parts[1].parse::<u32>().expect("week number");
        if parts[2] == "0" {
            format!("{}.{week:02}", parts[0])
        } else {
            format!("{}.{week:02}.{}", parts[0], parts[2])
        }
    } else {
        version.clone()
    };
    println!("cargo:rustc-env=OCS_APP_VERSION={app_version}");
    watch_git_inputs();
    let revision =
        git_output(&["rev-parse", "--short=12", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    // diff-index exits 1 for differences and 128 when there is no repository,
    // so only a real difference marks the build dirty.
    let dirty = std::process::Command::new("git")
        .args(["diff-index", "--quiet", "HEAD", "--"])
        .status()
        .ok()
        .is_some_and(|status| status.code() == Some(1));
    let commit = revision.clone();
    let revision = if dirty {
        format!("{revision}-dirty")
    } else {
        revision
    };
    println!("cargo:rustc-env=OCS_GIT_REV={revision}");

    // Builds from the main branch report how far past the release tag they
    // are. Release tags are `v` + the display version (scripts/release.py),
    // so counting first-parent commits from that tag ties the distance to
    // the version the app already claims. Shallow clones and source
    // tarballs have no tag to count from and fall back to the hash alone.
    let tag_distance = git_output(&[
        "rev-list",
        "--count",
        "--first-parent",
        &format!("v{app_version}..HEAD"),
    ])
    .and_then(|value| value.parse::<u64>().ok());
    println!(
        "cargo:rustc-env=OCS_TAG_DISTANCE={}",
        tag_distance.map_or_else(|| "unknown".to_string(), |n| n.to_string())
    );
    let commit_date =
        git_output(&["log", "-1", "--format=%cs", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=OCS_COMMIT_DATE={commit_date}");

    // Semver-style build metadata: empty for a clean build on the release
    // tag, `+N.g<hash>` for N first-parent commits past it, `+g<hash>` when
    // the distance is unknown, and `.dirty` appended for uncommitted changes.
    let short_hash = (commit != "unknown").then(|| commit[..commit.len().min(8)].to_string());
    let mut metadata_parts: Vec<String> = Vec::new();
    match (tag_distance, &short_hash) {
        (Some(0), _) => {}
        (Some(n), Some(hash)) => {
            metadata_parts.push(n.to_string());
            metadata_parts.push(format!("g{hash}"));
        }
        (Some(n), None) => metadata_parts.push(n.to_string()),
        (None, Some(hash)) => metadata_parts.push(format!("g{hash}")),
        (None, None) => {}
    }
    if dirty {
        metadata_parts.push("dirty".to_string());
    }
    let build_metadata = if metadata_parts.is_empty() {
        String::new()
    } else {
        format!("+{}", metadata_parts.join("."))
    };
    println!("cargo:rustc-env=OCS_BUILD_METADATA={build_metadata}");
    let full_version = format!("{app_version}{build_metadata}");
    println!("cargo:rustc-env=OCS_FULL_VERSION={full_version}");
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=OCS_BUILD_PROFILE={profile}");
    // Bounded selection UI construction exceeds the 1 MiB default in release;
    // link every profile explicitly because rustflags may be overridden/bypassed.
    if std::env::var("TARGET").is_ok_and(|t| t.ends_with("-pc-windows-msvc")) {
        println!("cargo:rustc-link-arg-bin=OpenCADStudio=/STACK:16777216");
    }
    let mut features: Vec<String> = std::env::vars()
        .filter_map(|(name, value)| {
            (value == "1")
                .then(|| name.strip_prefix("CARGO_FEATURE_").map(str::to_owned))
                .flatten()
        })
        .collect();
    features.sort();
    println!(
        "cargo:rustc-env=OCS_BUILD_FEATURES={}",
        if features.is_empty() {
            "none".to_string()
        } else {
            features.join(",")
        }
    );

    write_command_summaries();

    write_icon_pairs();

    // Release builds generate the icon before compiling.
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=packaging/windows/AppIcon.ico");
        if Path::new("packaging/windows/AppIcon.ico").exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("packaging/windows/AppIcon.ico");
            res.set("ProductVersion", &full_version);
            res.set("FileVersion", &app_version);
            if let Err(e) = res.compile() {
                println!("cargo:warning=failed to embed Windows icon: {e}");
            }
        }
    }
}

/// Pair every icon drawn for 32 px (`assets/icons/32/<p>`) with its 16 px
/// drawing (`assets/icons/<p>`) in `$OUT_DIR/icon_pairs.rs`, so the UI can
/// find the large art from the small art's bytes.
fn write_icon_pairs() {
    println!("cargo:rerun-if-changed=assets/icons");
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let icons = manifest.join("assets").join("icons");
    let large_root = icons.join("32");
    let mut relative = Vec::new();
    collect_svgs(&large_root, &large_root, &mut relative);
    relative.sort();

    let mut source = String::from("pub static ICON_PAIRS: &[(&[u8], &[u8])] = &[\n");
    for path in &relative {
        let small = icons.join(path);
        if !small.is_file() {
            println!(
                "cargo:warning=assets/icons/32/{} has no 16 px partner",
                path.display()
            );
            continue;
        }
        let large = large_root.join(path);
        source.push_str(&format!(
            "    (include_bytes!({:?}), include_bytes!({:?})),\n",
            small.display().to_string(),
            large.display().to_string()
        ));
    }
    source.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("icon_pairs.rs");
    std::fs::write(out, source).expect("write icon_pairs.rs");
}

/// Every `.svg` below `dir`, as a path relative to `root`.
fn collect_svgs(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_svgs(root, &path, out);
        } else if path.extension().is_some_and(|ext| ext == "svg") {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_path_buf());
            }
        }
    }
}

/// One-line summaries of commands and system variables, taken from the
/// `summary:` front matter of the wiki source pages, as a sorted
/// `(NAME, summary)` table for the command line's suggestion list.
fn write_command_summaries() {
    let mut entries: Vec<(String, String)> = Vec::new();
    for dir in ["docs/wiki/commands", "docs/wiki/sysvars"] {
        println!("cargo:rerun-if-changed={dir}");
        let Ok(files) = std::fs::read_dir(dir) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().is_none_or(|ext| ext != "md") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let summary = text
                .lines()
                .skip(1)
                .take_while(|line| *line != "---")
                .find_map(|line| line.strip_prefix("summary:"))
                .map(|s| s.trim().replace('`', ""));
            if let Some(summary) = summary.filter(|s| !s.is_empty()) {
                entries.push((name.to_ascii_uppercase(), summary));
            }
        }
    }
    entries.sort();
    entries.dedup_by(|a, b| a.0 == b.0);
    let mut out = String::from("&[\n");
    for (name, summary) in &entries {
        out.push_str(&format!("    ({name:?}, {summary:?}),\n"));
    }
    out.push_str("]\n");
    let path = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("command_summaries.rs");
    std::fs::write(path, out).expect("write command summaries");
}

/// Watch actual Git paths: `.git` is a file in linked worktrees. Missing
/// inputs make Cargo rerun the script on every build, even with no changes.
fn watch_git_inputs() {
    // Source archives have no Git metadata. Avoid Cargo's fallback scan of
    // the entire package, without watching a nonexistent `.git/HEAD`.
    println!("cargo:rerun-if-changed=build.rs");
    let Some(head_path) = git_output(&["rev-parse", "--git-path", "HEAD"]) else {
        return;
    };
    println!("cargo:rerun-if-changed={head_path}");
    let Ok(head) = std::fs::read_to_string(&head_path) else {
        return;
    };
    let Some(reference) = head.trim().strip_prefix("ref: ") else {
        return;
    };
    let Some(reference_path) = git_output(&["rev-parse", "--git-path", reference]) else {
        return;
    };
    let mut reference_path = PathBuf::from(reference_path);
    if !reference_path.exists() {
        if let Some(packed) = git_output(&["rev-parse", "--git-path", "packed-refs"]) {
            if Path::new(&packed).exists() {
                println!("cargo:rerun-if-changed={packed}");
            }
        }
        // A new commit can create a loose ref without changing packed-refs.
        // Watch its nearest existing parent until the loose ref exists.
        while !reference_path.exists() {
            if !reference_path.pop() {
                return;
            }
        }
    }
    // Do not watch the index: `git status` can rewrite it without a change.
    println!("cargo:rerun-if-changed={}", reference_path.display());
}

/// Runs `git` with `args` and returns trimmed stdout, or `None` when git is
/// missing, fails (no repository, unknown tag), or prints nothing.
fn git_output(args: &[&str]) -> Option<String> {
    std::process::Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
