//! Repo automation for firecrawl-pdfium.
//!
//! Subcommands:
//!
//! - `fetch-pdfium [--platform <slug>|--all] [--print-path]`
//!   Download the pinned PDFium archive(s) from `pdfium.lock.json`, verify
//!   sha256, and extract to `target/pdfium/<slug>/`. With `--print-path`,
//!   print only the library directory (for shell interpolation).
//!
//! - `repackage --platform <slug>|--all [--out <dir>] [--tag <tag>]`
//!   Build the release assets this repository publishes: the verified
//!   upstream archive contents (`lib/`, `include/`, `licenses/`, `LICENSE`,
//!   `VERSION`, `args.gn`) plus a `PROVENANCE.json` recording the upstream
//!   release, asset, and sha256. Writes `<out>/firecrawl-pdfium-<slug>.tgz`
//!   (byte-reproducible; see `create_deterministic_tgz`) plus `SHA256SUMS`
//!   and `ARTIFACT-SIZES.txt` covering everything in `<out>`.
//!
//! - `pin chromium/NNNN`
//!   Re-pin `pdfium.lock.json` to a new upstream release: downloads every
//!   platform asset, records fresh sha256 checksums, and rewrites the lock.
//!
//! Downloads shell out to `curl` and extraction to `tar`, both present on
//! all GitHub-hosted runners (including Windows) and dev machines; release
//! archives are created in-process so their bytes are identical on every
//! platform.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
struct Lock {
    release: String,
    pdfium_version: String,
    base_url: String,
    platforms: BTreeMap<String, PlatformPin>,
}

#[derive(Deserialize)]
struct PlatformPin {
    asset: String,
    sha256: String,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let root = repo_root()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut it = args.iter().map(String::as_str);

    match it.next() {
        Some("fetch-pdfium") => fetch_pdfium(&root, &args[1..]),
        Some("repackage") => repackage(&root, &args[1..]),
        Some("pin") => pin(&root, &args[1..]),
        Some(other) => Err(format!(
            "unknown subcommand `{other}`; see xtask/src/main.rs"
        )),
        None => Err("usage: cargo xtask <fetch-pdfium|repackage|pin> [options]".into()),
    }
}

/// `pin chromium/NNNN` — re-pin pdfium.lock.json to a new upstream release:
/// downloads every pinned platform's asset from the new tag, records fresh
/// sha256 checksums, reads the archive's VERSION metadata, and rewrites the
/// lock file. Review the diff, run the test suite, then commit.
fn pin(root: &Path, args: &[String]) -> Result<(), String> {
    let tag = args
        .first()
        .ok_or_else(|| "usage: cargo xtask pin chromium/NNNN".to_string())?;
    if !tag.starts_with("chromium/") || tag.len() <= "chromium/".len() {
        return Err(format!("expected a tag like chromium/7988, got `{tag}`"));
    }

    let lock = load_lock(root)?;
    let base_url = format!("https://github.com/bblanchon/pdfium-binaries/releases/download/{tag}");
    let work = root.join("target").join("pin");
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;

    let mut platforms = serde_json::Map::new();
    let mut pdfium_version = String::new();
    for (slug, old_pin) in &lock.platforms {
        let url = format!("{base_url}/{}", old_pin.asset);
        let archive = work.join(&old_pin.asset);
        eprintln!("downloading {url}");
        let status = Command::new("curl")
            .args(["-fsSL", "--retry", "3", "--retry-delay", "2", "-o"])
            .arg(&archive)
            .arg(&url)
            .status()
            .map_err(|e| format!("running curl: {e}"))?;
        if !status.success() {
            return Err(format!("curl failed for {url} — does release {tag} exist?"));
        }
        let sha = sha256_file(&archive)?;
        eprintln!("  {slug}: {sha}");
        platforms.insert(
            slug.clone(),
            serde_json::json!({ "asset": old_pin.asset, "sha256": sha }),
        );

        if pdfium_version.is_empty() {
            let stage = work.join("inspect");
            extract(&archive, &stage)?;
            let version_file = stage.join("VERSION");
            let raw = fs::read_to_string(&version_file)
                .map_err(|e| format!("reading {}: {e}", version_file.display()))?;
            let field = |k: &str| {
                raw.lines()
                    .find_map(|l| l.strip_prefix(&format!("{k}=")))
                    .unwrap_or("0")
                    .trim()
                    .to_string()
            };
            pdfium_version = format!(
                "{}.{}.{}.{}",
                field("MAJOR"),
                field("MINOR"),
                field("BUILD"),
                field("PATCH")
            );
        }
    }

    let new_lock = serde_json::json!({
        "comment": "Pinned upstream PDFium binaries. Update with: cargo xtask pin chromium/NNNN. Checksums are recorded at pin time because upstream publishes none; every download is verified against them.",
        "upstream": "bblanchon/pdfium-binaries",
        "release": tag,
        "pdfium_version": pdfium_version,
        "base_url": base_url,
        "platforms": platforms,
    });
    let path = root.join("pdfium.lock.json");
    fs::write(
        &path,
        serde_json::to_string_pretty(&new_lock).expect("static json") + "\n",
    )
    .map_err(|e| e.to_string())?;
    eprintln!(
        "pinned {tag} (PDFium {pdfium_version}) -> {}\nNext: cargo xtask fetch-pdfium && cargo test, then commit.",
        path.display()
    );
    Ok(())
}

fn repo_root() -> Result<PathBuf, String> {
    // xtask always runs via `cargo xtask` (alias) or `cargo run -p xtask`,
    // so the manifest dir is <root>/xtask.
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| "CARGO_MANIFEST_DIR not set; run via cargo".to_string())?;
    Ok(PathBuf::from(manifest)
        .parent()
        .expect("xtask has a parent directory")
        .to_path_buf())
}

fn load_lock(root: &Path) -> Result<Lock, String> {
    let path = root.join("pdfium.lock.json");
    let data = fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    serde_json::from_str(&data).map_err(|e| format!("parsing {}: {e}", path.display()))
}

fn host_slug() -> Result<&'static str, String> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "mac-arm64",
        ("macos", "x86_64") => "mac-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("linux", "x86_64") => "linux-x64",
        ("windows", "x86_64") => "win-x64",
        (os, arch) => return Err(format!("no pinned PDFium binary for host {os}/{arch}")),
    })
}

fn parse_platforms(lock: &Lock, args: &[String]) -> Result<Vec<String>, String> {
    let mut platforms = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--platform" => {
                let slug = args
                    .get(i + 1)
                    .ok_or_else(|| "--platform requires a value".to_string())?;
                if !lock.platforms.contains_key(slug) {
                    return Err(format!(
                        "unknown platform `{slug}`; pinned: {}",
                        lock.platforms
                            .keys()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                platforms.push(slug.clone());
                i += 2;
            }
            "--all" => {
                platforms = lock.platforms.keys().cloned().collect();
                i += 1;
            }
            _ => i += 1,
        }
    }
    if platforms.is_empty() {
        platforms.push(host_slug()?.to_string());
    }
    Ok(platforms)
}

fn fetch_pdfium(root: &Path, args: &[String]) -> Result<(), String> {
    let lock = load_lock(root)?;
    let print_path = args.iter().any(|a| a == "--print-path");
    let platforms = parse_platforms(&lock, args)?;

    for slug in &platforms {
        let dest = root.join("target").join("pdfium").join(slug);
        let archive = download_verified(root, &lock, slug)?;
        extract(&archive, &dest)?;
        let lib_dir = lib_dir_for(&dest, slug);
        if print_path {
            println!("{}", lib_dir.display());
        } else {
            eprintln!(
                "fetched PDFium {} ({}) -> {}",
                lock.pdfium_version,
                slug,
                lib_dir.display()
            );
        }
    }
    Ok(())
}

/// Windows archives put the DLL in `bin/`, everything else uses `lib/`.
fn lib_dir_for(dest: &Path, slug: &str) -> PathBuf {
    if slug.starts_with("win") {
        dest.join("bin")
    } else {
        dest.join("lib")
    }
}

/// Downloads (with cache) the pinned asset for `slug` and verifies its
/// sha256 against the lock file. Returns the archive path.
fn download_verified(root: &Path, lock: &Lock, slug: &str) -> Result<PathBuf, String> {
    let pin = &lock.platforms[slug];
    let cache_dir = root.join("target").join("pdfium").join("download");
    fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;
    let archive = cache_dir.join(&pin.asset);

    if !archive.is_file() || sha256_file(&archive)? != pin.sha256 {
        let url = format!("{}/{}", lock.base_url, pin.asset);
        eprintln!("downloading {url}");
        let tmp = archive.with_extension("tgz.partial");
        let status = Command::new("curl")
            .args(["-fsSL", "--retry", "3", "--retry-delay", "2", "-o"])
            .arg(&tmp)
            .arg(&url)
            .status()
            .map_err(|e| format!("running curl: {e}"))?;
        if !status.success() {
            return Err(format!("curl failed for {url}"));
        }
        fs::rename(&tmp, &archive).map_err(|e| e.to_string())?;
    }

    let actual = sha256_file(&archive)?;
    if actual != pin.sha256 {
        // Remove the poisoned cache entry so a retry re-downloads.
        let _ = fs::remove_file(&archive);
        return Err(format!(
            "sha256 mismatch for {}: expected {}, got {actual}. \
             Upstream asset changed or download corrupted; refusing to use it.",
            pin.asset, pin.sha256
        ));
    }
    Ok(archive)
}

fn extract(archive: &Path, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        fs::remove_dir_all(dest).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let status = Command::new("tar")
        .arg("xzf")
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .status()
        .map_err(|e| format!("running tar: {e}"))?;
    if !status.success() {
        return Err(format!("tar failed extracting {}", archive.display()));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn repackage(root: &Path, args: &[String]) -> Result<(), String> {
    let lock = load_lock(root)?;
    let platforms = parse_platforms(&lock, args)?;

    let mut out_dir = root.join("dist");
    let mut tag = String::from("untagged");
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                out_dir = PathBuf::from(
                    args.get(i + 1)
                        .ok_or_else(|| "--out requires a value".to_string())?,
                );
                i += 2;
            }
            "--tag" => {
                tag = args
                    .get(i + 1)
                    .ok_or_else(|| "--tag requires a value".to_string())?
                    .clone();
                i += 2;
            }
            _ => i += 1,
        }
    }
    fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;

    for slug in &platforms {
        let pin = &lock.platforms[slug];
        let archive = download_verified(root, &lock, slug)?;
        let stage = root.join("target").join("repackage").join(slug);
        extract(&archive, &stage)?;

        // Everything upstream ships (lib/include/licenses/LICENSE/VERSION/
        // args.gn) stays verbatim — that is what satisfies the third-party
        // notice obligations. We add provenance metadata on top.
        let provenance = serde_json::json!({
            "name": format!("firecrawl-pdfium native archive ({slug})"),
            "repackaged_by": "firecrawl/pdfium-rs",
            "repackage_tag": tag,
            "upstream": {
                "project": "bblanchon/pdfium-binaries",
                "release": lock.release,
                "pdfium_version": lock.pdfium_version,
                "asset": pin.asset,
                "url": format!("{}/{}", lock.base_url, pin.asset),
                "sha256": pin.sha256,
            },
            "license_note": "See LICENSE and licenses/ for PDFium and bundled third-party component licenses.",
        });
        fs::write(
            stage.join("PROVENANCE.json"),
            serde_json::to_string_pretty(&provenance).expect("static json"),
        )
        .map_err(|e| e.to_string())?;

        let out_name = format!("firecrawl-pdfium-{slug}.tgz");
        let out_path = out_dir.join(&out_name);
        create_deterministic_tgz(&stage, &out_path)?;
        eprintln!("wrote {}", out_path.display());
    }

    // SHA256SUMS over every file in the output dir (idempotent rebuild), with
    // byte sizes recorded alongside in ARTIFACT-SIZES.txt for the release
    // notes.
    write_sums_and_sizes(&out_dir)?;
    Ok(())
}

/// Creates `out_path` as a byte-reproducible .tgz of `stage`: two runs over
/// identical input trees produce identical archives, on any platform.
///
/// Reproducibility is load-bearing for the release pipeline — published
/// asset checksums must survive a workflow re-run (consumers pin them, and
/// the upload step refuses to overwrite an asset with different bytes). The
/// archive is written in-process (`tar` + `flate2` crates, pure Rust) so
/// there is no BSD-vs-GNU-tar variance:
///
/// - entries are added in sorted path order (not readdir order);
/// - entry names have no `./` prefix;
/// - `tar::HeaderMode::Deterministic` zeroes ownership and timestamps and
///   normalizes permissions;
/// - the gzip stream has no name/mtime metadata and a fixed compression
///   level, and flate2's pure-Rust backend is platform-independent.
fn create_deterministic_tgz(stage: &Path, out_path: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    collect_files(stage, stage, &mut files)?;
    files.sort();

    let out =
        fs::File::create(out_path).map_err(|e| format!("create {}: {e}", out_path.display()))?;
    let gz = flate2::GzBuilder::new().write(out, flate2::Compression::new(9));
    let mut builder = tar::Builder::new(gz);
    builder.mode(tar::HeaderMode::Deterministic);

    for rel in &files {
        let abs = stage.join(rel);
        let mut f = fs::File::open(&abs).map_err(|e| format!("open {}: {e}", abs.display()))?;
        // Forward slashes in entry names regardless of host OS.
        let name = rel
            .iter()
            .map(|c| c.to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        builder
            .append_file(&name, &mut f)
            .map_err(|e| format!("tar append {name}: {e}"))?;
    }
    let gz = builder
        .into_inner()
        .map_err(|e| format!("tar finish: {e}"))?;
    gz.finish().map_err(|e| format!("gzip finish: {e}"))?;
    Ok(())
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else {
            out.push(
                path.strip_prefix(root)
                    .expect("path is under root")
                    .to_path_buf(),
            );
        }
    }
    Ok(())
}

/// Writes SHA256SUMS and ARTIFACT-SIZES.txt covering every file in `out_dir`.
fn write_sums_and_sizes(out_dir: &Path) -> Result<(), String> {
    let mut sums = String::new();
    let mut sizes = String::new();
    let mut entries: Vec<_> = fs::read_dir(out_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .map_or(true, |n| n != "SHA256SUMS" && n != "ARTIFACT-SIZES.txt")
        })
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        sums.push_str(&format!("{}  {}\n", sha256_file(&path)?, name));
        let bytes = fs::metadata(&path).map_err(|e| e.to_string())?.len();
        sizes.push_str(&format!("{bytes}\t{name}\n"));
    }
    fs::write(out_dir.join("SHA256SUMS"), sums).map_err(|e| e.to_string())?;
    fs::write(out_dir.join("ARTIFACT-SIZES.txt"), sizes).map_err(|e| e.to_string())?;
    eprintln!(
        "wrote {} and ARTIFACT-SIZES.txt",
        out_dir.join("SHA256SUMS").display()
    );
    Ok(())
}
