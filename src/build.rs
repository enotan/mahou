use crate::config::{
    add_auth_header, build_dir, distfiles_dir, http_client, install_db_dir, stage_dir, stage_root,
};

use crate::package::{InstallRecord, Package};

use chrono::Utc;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs as unix_fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::features::expand_build_step;

pub fn fetch_package(package: &Package) -> Result<(), String> {
    fs::create_dir_all(distfiles_dir())
        .map_err(|error| format!("Failed to create distfiles directory: {}", error))?;

    let filename = source_filename(package)?;

    let output_path = format!("{}/{}", distfiles_dir(), filename);

    let partial_path = format!("{}.part", output_path);

    if fs::metadata(&output_path).is_ok() {
        println!("Already fetched: {}", output_path);
        return verify_package(package);
    }

    println!("Fetching {} from {}", package.name, package.source);

    let client = http_client()?;

    let response = add_auth_header(client.get(&package.source), &package.source)
        .send()
        .map_err(|error| format!("Failed to download {}: {}", package.name, error))?
        .error_for_status()
        .map_err(|error| format!("Download failed for {}: {}", package.source, error))?;

    download_with_progress(response, &partial_path, &package.name)?;

    let actual = sha256_file(&partial_path)?;

    if actual != package.sha256 {
        let _ = fs::remove_file(&partial_path);

        return Err(format!(
            "Checksum mismatch for {}\n expected: {}\n   actual: {}",
            package.name, package.sha256, actual
        ));
    }

    fs::rename(&partial_path, &output_path)
        .map_err(|error| format!("Failed to save {}: {}", output_path, error))?;

    println!("Saved to {}", output_path);
    println!("Verified: {}", output_path);

    Ok(())
}

fn download_with_progress(
    mut response: reqwest::blocking::Response,
    output_path: &str,
    package_name: &str,
) -> Result<(), String> {
    let mut file = fs::File::create(output_path)
        .map_err(|error| format!("Failed to create {}: {}", output_path, error))?;

    let total = response.content_length();
    let mut downloaded = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];

    let frames = ["✦", "✧", "◇", "◆"];
    let mut frame_index = 0_usize;
    let mut last_draw = Instant::now();

    loop {
        let bytes_read = response
            .read(&mut buffer)
            .map_err(|error| format!("Failed to read response for {}: {}", package_name, error))?;

        if bytes_read == 0 {
            break;
        }

        file.write_all(&buffer[..bytes_read])
            .map_err(|error| format!("Failed to write {}: {}", output_path, error))?;

        downloaded += bytes_read as u64;

        if last_draw.elapsed() >= Duration::from_millis(100) {
            let frame = frames[frame_index % frames.len()];
            frame_index += 1;

            match total {
                Some(total) => {
                    print!(
                        "\rFetching {} {} {:.1} MiB / {:.1} MiB",
                        package_name,
                        frame,
                        downloaded as f64 / 1024.0 / 1024.0,
                        total as f64 / 1024.0 / 1024.0
                    );
                }
                None => {
                    print!(
                        "\rFetching {} {} {:.1} MiB",
                        package_name,
                        frame,
                        downloaded as f64 / 1024.0 / 1024.0
                    );
                }
            }

            std::io::stdout()
                .flush()
                .map_err(|error| format!("Failed to flush stdout: {}", error))?;

            last_draw = Instant::now();
        }
    }

    println!();

    Ok(())
}

pub fn verify_package(package: &Package) -> Result<(), String> {
    let filename = source_filename(package)?;
    let path = format!("{}/{}", distfiles_dir(), filename);
    let actual = sha256_file(&path)?;

    if actual != package.sha256 {
        return Err(format!(
            "Checksum mismatch for {}\n expected: {}\n   actual: {}",
            package.name, package.sha256, actual
        ));
    }

    println!("Verified: {}", path);

    Ok(())
}

fn source_filename(package: &Package) -> Result<&str, String> {
    let filename = package
        .source
        .rsplit('/')
        .next()
        .ok_or_else(|| format!("Invalid source URL for {}", package.name))?;

    if filename.is_empty() {
        return Err(format!("Invalid source URL for {}", package.name));
    }

    Ok(filename)
}

pub fn extract_package(package: &Package) -> Result<(), String> {
    fetch_package(package)?;

    fs::create_dir_all(build_dir())
        .map_err(|error| format!("Failed to create build directory: {}", error))?;

    let filename = source_filename(package)?;
    let archive_path = format!("{}/{}", distfiles_dir(), filename);
    let archive_source_dir = format!("{}/{}", build_dir(), package.source_dir);
    let package_source_dir = package_build_dir(package);

    if fs::metadata(&package_source_dir).is_ok() {
        println!("Already extracted: {}", package_source_dir);
        return Ok(());
    }

    println!("Extracting {} into build/", archive_path);

    let status = Command::new("tar")
        .arg("-xf")
        .arg(&archive_path)
        .arg("-C")
        .arg(build_dir())
        .status()
        .map_err(|error| format!("Failed to run tar: {}", error))?;

    if !status.success() {
        return Err(format!("tar failed while extracting {}", archive_path));
    }

    if fs::metadata(&archive_source_dir).is_err() {
        return Err(format!(
            "Expected source directory '{}' not found after extraction",
            archive_source_dir
        ));
    }

    fs::rename(&archive_source_dir, &package_source_dir).map_err(|error| {
        format!(
            "Failed to isolate build directory '{}' as '{}': {}",
            archive_source_dir, package_source_dir, error
        )
    })?;

    println!("Extracted: {}", package_source_dir);
    Ok(())
}

fn run_build_step(package: &Package, step: &str) -> Result<(), String> {
    let source_dir = package_build_dir(package);
    let destdir = stage_dir(package);

    fs::create_dir_all(&destdir)
        .map_err(|error| format!("Failed to create stage directory '{}': {}", destdir, error))?;

    println!("Running build step for {}: {}", package.name, step);

    let stage_prefixes = staged_prefixes();

    let pkg_config_defaults = build_pkg_config_defaults(package, &stage_prefixes);
    let pkg_config_path = build_profile_path(package, "PKG_CONFIG_PATH", &pkg_config_defaults);

    let library_defaults = build_library_defaults(package, &stage_prefixes);

    let library_path = build_profile_path(package, "LIBRARY_PATH", &library_defaults);
    let ld_library_path = build_profile_path(package, "LD_LIBRARY_PATH", &library_defaults);

    let mut path_defaults = Vec::new();
    for prefix in &stage_prefixes {
        path_defaults.push(format!("{}/bin", prefix));
        path_defaults.push(format!("{}/sbin", prefix));
    }
    path_defaults.extend([
        "/opt/rustc/bin".to_string(),
        "/usr/local/sbin".to_string(),
        "/usr/local/bin".to_string(),
        "/usr/sbin".to_string(),
        "/usr/bin".to_string(),
        "/sbin".to_string(),
        "/bin".to_string(),
    ]);

    let path = build_env_path("PATH", &path_defaults);

    let mut cmake_prefix_defaults = stage_prefixes;
    cmake_prefix_defaults.push("/usr".to_string());

    let cmake_prefix_path = build_env_path("CMAKE_PREFIX_PATH", &cmake_prefix_defaults);

    let profile_env = build_profile_env(package);

    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(step)
        .current_dir(&source_dir)
        .env("MAHOU_DESTDIR", &destdir)
        .env("PATH", path)
        .env("PKG_CONFIG_PATH", pkg_config_path)
        .env("LIBRARY_PATH", library_path)
        .env("LD_LIBRARY_PATH", ld_library_path)
        .env("CMAKE_PREFIX_PATH", cmake_prefix_path);

    for (name, value) in profile_env {
        command.env(name, value);
    }

    let status = command
        .status()
        .map_err(|error| format!("Failed to run build step for {}: {}", step, error))?;

    if !status.success() {
        return Err(format!("Build step failed for {}: {}", package.name, step));
    }

    Ok(())
}

fn build_env_path(name: &str, defaults: &[String]) -> String {
    let mut paths = defaults.to_vec();

    if let Ok(existing) = env::var(name) {
        for path in existing.split(':') {
            if !path.is_empty() && !paths.iter().any(|existing| existing == path) {
                paths.push(path.to_string());
            }
        }
    }

    paths.join(":")
}

fn build_profile_path(package: &Package, name: &str, defaults: &[String]) -> String {
    match package.build_profile.as_str() {
        "lib32" => defaults.join(":"),
        _ => build_env_path(name, defaults),
    }
}

fn build_profile_env(package: &Package) -> Vec<(&'static str, String)> {
    match package.build_profile.as_str() {
        "lib32" => vec![
            ("CFLAGS", merge_flags("CFLAGS", "-m32")),
            ("CXXFLAGS", merge_flags("CXXFLAGS", "-m32")),
            ("LDFLAGS", merge_flags("LDFLAGS", "-m32")),
            (
                "PKG_CONFIG_LIBDIR",
                "/usr/lib32/pkgconfig:/usr/share/pkgconfig".to_string(),
            ),
        ],
        "native" => Vec::new(),
        other => {
            eprintln!(
                "Warning: unknown build profile '{}' for package '{}'; using default profile.",
                other, package.name
            );
            Vec::new()
        }
    }
}

fn build_library_defaults(package: &Package, stage_prefixes: &[String]) -> Vec<String> {
    let mut paths = Vec::new();

    match package.build_profile.as_str() {
        "lib32" => {
            for prefix in stage_prefixes {
                paths.push(format!("{}/lib32", prefix));
            }

            paths.push("/usr/lib32".to_string());
        }
        _ => {
            for prefix in stage_prefixes {
                paths.push(format!("{}/lib64", prefix));
                paths.push(format!("{}/lib", prefix));
            }

            paths.push("/usr/lib64".to_string());
            paths.push("/usr/lib".to_string());
        }
    }

    paths
}

fn build_pkg_config_defaults(package: &Package, stage_prefixes: &[String]) -> Vec<String> {
    let mut paths = Vec::new();

    match package.build_profile.as_str() {
        "lib32" => {
            for prefix in stage_prefixes {
                paths.push(format!("{}/lib32/pkgconfig", prefix));
                paths.push(format!("{}/share/pkgconfig", prefix));
            }

            paths.push("/usr/lib32/pkgconfig".to_string());
            paths.push("/usr/share/pkgconfig".to_string());
        }
        _ => {
            for prefix in stage_prefixes {
                paths.push(format!("{}/lib64/pkgconfig", prefix));
                paths.push(format!("{}/lib/pkgconfig", prefix));
                paths.push(format!("{}/share/pkgconfig", prefix));
            }

            paths.push("/usr/lib64/pkgconfig".to_string());
            paths.push("/usr/lib/pkgconfig".to_string());
            paths.push("/usr/share/pkgconfig".to_string());
        }
    }

    paths
}

fn merge_flags(name: &str, flags: &str) -> String {
    match env::var(name) {
        Ok(existing) if !existing.trim().is_empty() => format!("{} {}", flags, existing),
        _ => flags.to_string(),
    }
}

fn staged_prefixes() -> Vec<String> {
    let Ok(entries) = fs::read_dir(stage_root()) else {
        return Vec::new();
    };

    let mut prefixes: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("usr"))
        .filter(|path| path.is_dir())
        .filter_map(|path| path.to_str().map(|path| path.to_string()))
        .collect();

    prefixes.sort();
    prefixes
}

pub fn build_package(package: &Package, flags: &[String]) -> Result<(), String> {
    if is_built(package) {
        println!("Already built: {}", package.name);
        return Ok(());
    }

    clean_stage(package)?;

    extract_package(package)?;

    println!("Building {} {}", package.name, package.version);

    for step in &package.build_steps {
        let expanded_step = expand_build_step(package, step, flags);
        run_build_step(package, &expanded_step)?;
    }

    mark_built(package)?;

    println!("Built: {}", package.name);

    Ok(())
}

fn sha256_file(path: &str) -> Result<String, String> {
    let contents = fs::read(path).map_err(|error| format!("Failed to read {}: {}", path, error))?;

    Ok(sha256_bytes(&contents))
}

pub fn sha256_bytes(contents: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(contents);

    hex::encode(hasher.finalize())
}

fn build_marker_path(package: &Package) -> String {
    format!("{}/.mahou-built", stage_dir(package))
}

fn is_built(package: &Package) -> bool {
    let marker_path = build_marker_path(package);

    let Ok(contents) = fs::read_to_string(marker_path) else {
        return false;
    };

    contents.trim() == format!("{} {}", package.name, package.version)
}

fn mark_built(package: &Package) -> Result<(), String> {
    let marker_path = build_marker_path(package);

    fs::write(
        &marker_path,
        format!("{} {}\n", package.name, package.version),
    )
    .map_err(|error| format!("Failed to write build marker {}: {}", marker_path, error))?;

    Ok(())
}

pub fn clean_build(package: &Package) -> Result<(), String> {
    let path = package_build_dir(package);

    if fs::metadata(&path).is_ok() {
        fs::remove_dir_all(&path)
            .map_err(|error| format!("Failed to remove build directory {}: {}", path, error))?;
    }

    Ok(())
}

pub fn clean_stage(package: &Package) -> Result<(), String> {
    let path = stage_dir(package);

    if fs::metadata(&path).is_ok() {
        fs::remove_dir_all(&path)
            .map_err(|error| format!("Failed to remove stage directory {}: {}", path, error))?;
    }

    Ok(())
}

fn package_build_dir(package: &Package) -> String {
    format!(
        "{}/{}-{}-{}",
        build_dir(),
        package.source_dir,
        sanitize_build_component(&package.build_profile),
        sanitize_build_component(&package.name)
    )
}

fn sanitize_build_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '+') {
                character
            } else {
                '-'
            }
        })
        .collect()
}

pub fn collect_files(root: &str) -> Result<Vec<String>, String> {
    let mut files = Vec::new();

    collect_files_recursive(root, root, &mut files)?;

    files.sort();

    Ok(files)
}

fn collect_files_recursive(
    base: &str,
    current: &str,
    files: &mut Vec<String>,
) -> Result<(), String> {
    let entries = fs::read_dir(current)
        .map_err(|error| format!("Failed to read directory {}: {}", current, error))?;

    for entry in entries {
        let entry = entry.map_err(|error| format!("Failed to read directory entry: {}", error))?;
        let path = entry.path();
        let path_string = path.to_string_lossy().to_string();

        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("Failed to inspect {}: {}", path.display(), error))?;

        if metadata.is_dir() {
            collect_files_recursive(base, &path_string, files)?;
        } else if metadata.is_file() || metadata.file_type().is_symlink() {
            let relative = path
                .strip_prefix(base)
                .map_err(|error| format!("Failed to make relative path: {}", error))?
                .to_string_lossy()
                .to_string();

            files.push(format!("/{}", relative));
        }
    }

    Ok(())
}

pub fn install_staged_files(package: &Package) -> Result<Vec<String>, String> {
    let stage = stage_dir(package);
    let files = collect_files(&stage)?;

    check_file_conflicts(package, &files)?;

    for file in &files {
        let source = format!("{}{}", stage, file);
        let target = file.to_string();

        if let Some(parent) = std::path::Path::new(&target).parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!("Failed to create directory {}: {}", parent.display(), error)
            })?;
        }

        let metadata = fs::symlink_metadata(&source)
            .map_err(|error| format!("Failed to inspect {}: {}", source, error))?;

        if metadata.file_type().is_symlink() {
            let link_target = fs::read_link(&source)
                .map_err(|error| format!("Failed to read symlink {}: {}", source, error))?;

            if fs::symlink_metadata(&target).is_ok() {
                fs::remove_file(&target)
                    .map_err(|error| format!("Failed to replace symlink {}: {}", target, error))?;
            }

            unix_fs::symlink(&link_target, &target)
                .map_err(|error| format!("Failed to create symlink {}: {}", target, error))?;
        } else {
            install_regular_file(&source, &target, &metadata)?;
        }
    }

    Ok(files)
}

fn install_regular_file(source: &str, target: &str, metadata: &fs::Metadata) -> Result<(), String> {
    let target_path = std::path::Path::new(target);
    let parent = target_path
        .parent()
        .ok_or_else(|| format!("Invalid install target: {}", target))?;

    let filename = target_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("Invalid install target: {}", target))?;

    let temp_target = parent.join(format!(
        ".mahou-install-{}-{}",
        std::process::id(),
        filename
    ));

    if temp_target.exists() {
        fs::remove_file(&temp_target).map_err(|error| {
            format!(
                "Failed to remove temporary file {}: {}",
                temp_target.display(),
                error
            )
        })?;
    }

    fs::copy(source, &temp_target).map_err(|error| {
        format!(
            "Failed to copy {} to temporary file {}: {}",
            source,
            temp_target.display(),
            error
        )
    })?;

    fs::set_permissions(&temp_target, metadata.permissions()).map_err(|error| {
        let _ = fs::remove_file(&temp_target);
        format!(
            "Failed to set permissions on {}: {}",
            temp_target.display(),
            error
        )
    })?;

    fs::rename(&temp_target, target).map_err(|error| {
        let _ = fs::remove_file(&temp_target);
        format!("Failed to install {} to {}: {}", source, target, error)
    })?;

    Ok(())
}

fn write_install_record(
    package: &Package,
    files: &[String],
    install_reason: &str,
) -> Result<(), String> {
    write_install_record_with_version(
        &package.name,
        &package.version,
        &package.build_profile,
        install_reason,
        files,
    )
}

pub fn write_install_record_with_version(
    name: &str,
    version: &str,
    build_profile: &str,
    install_reason: &str,
    files: &[String],
) -> Result<(), String> {
    fs::create_dir_all(install_db_dir())
        .map_err(|error| format!("Failed to create install database: {}", error))?;

    let path = install_record_path_for_name(name);

    let mut contents = String::new();
    contents.push_str(&format!("name = \"{}\"\n", name));

    let installed_at = Utc::now().to_rfc3339();

    contents.push_str(&format!("version = \"{}\"\n", version));
    contents.push_str(&format!("build_profile = \"{}\"\n", build_profile));
    contents.push_str(&format!("installed_at = \"{}\"\n", installed_at));
    contents.push_str(&format!("install_reason = \"{}\"\n", install_reason));
    contents.push_str("files = [\n");

    for file in files {
        contents.push_str(&format!("    \"{}\",\n", file));
    }

    contents.push_str("]\n");

    fs::write(&path, contents)
        .map_err(|error| format!("Failed to write install record {}: {}", path, error))?;

    Ok(())
}

pub fn adopt_package(package: &Package, as_current: bool, dry_run: bool) -> Result<bool, String> {
    if load_install_record(&package.name)?.is_some() {
        println!("Already tracked: {}", package.name);
        return Ok(false);
    }

    let detection = detect_host_package(package);

    if !detection.found {
        println!("Not found: {}", package.name);
        return Ok(false);
    }

    let adopt_version = match detection.version.as_deref() {
        Some(version) if version == package.version => package.version.as_str(),
        Some(version) if as_current => {
            println!(
                "Adopting {} as {} (detected {})",
                package.name, package.version, version
            );
            package.version.as_str()
        }
        Some(version) => {
            println!(
                "Found {} {}, but recipe is {}; skipped (use --as-current to trust it)",
                package.name, version, package.version
            );
            return Ok(false);
        }
        None if as_current => {
            println!(
                "Adopting {} as {} ({})",
                package.name, package.version, detection.reason
            );
            package.version.as_str()
        }
        None => {
            println!(
                "Found {}, but could not detect version; skipped (use --as-current to trust it)",
                package.name
            );
            return Ok(false);
        }
    };

    if dry_run {
        println!("Would adopt: {} {}", package.name, adopt_version);
        return Ok(true);
    }

    write_install_record_with_version(
        &package.name,
        adopt_version,
        &package.build_profile,
        "adopted",
        &[],
    )?;
    println!("Adopted: {} {}", package.name, adopt_version);
    Ok(true)
}

struct HostPackageDetection {
    found: bool,
    version: Option<String>,
    reason: String,
}

fn detect_host_package(package: &Package) -> HostPackageDetection {
    for pc_name in pkg_config_candidates(&package.name) {
        match pkg_config_version(&pc_name) {
            Ok(Some(version)) => {
                return HostPackageDetection {
                    found: true,
                    version: Some(version),
                    reason: format!("pkg-config {}", pc_name),
                };
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("Warning: failed to query pkg-config {}: {}", pc_name, error);
            }
        }
    }

    for path in host_file_candidates(&package.name) {
        if Path::new(&path).exists() {
            return HostPackageDetection {
                found: true,
                version: None,
                reason: format!("found {}", path),
            };
        }
    }

    HostPackageDetection {
        found: false,
        version: None,
        reason: "not found".to_string(),
    }
}

fn pkg_config_version(name: &str) -> Result<Option<String>, String> {
    let output = Command::new("pkg-config")
        .arg("--modversion")
        .arg(name)
        .output()
        .map_err(|error| format!("failed to run pkg-config: {}", error))?;

    if !output.status.success() {
        return Ok(None);
    }

    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if version.is_empty() {
        Ok(None)
    } else {
        Ok(Some(version))
    }
}

fn pkg_config_candidates(name: &str) -> Vec<String> {
    let mut candidates = vec![name.to_string(), name.to_lowercase()];

    match name {
        "atkmm" => candidates.push("atkmm-1.6".to_string()),
        "cairomm" => candidates.push("cairomm-1.0".to_string()),
        "freetype" => candidates.push("freetype2".to_string()),
        "glibmm" => candidates.push("glibmm-2.4".to_string()),
        "gtk+" => candidates.push("gtk+-3.0".to_string()),
        "gtkmm" => candidates.push("gtkmm-3.0".to_string()),
        "libsigc++" => candidates.push("sigc++-2.0".to_string()),
        "libX11" => candidates.push("x11".to_string()),
        "libXau" => candidates.push("xau".to_string()),
        "libXdmcp" => candidates.push("xdmcp".to_string()),
        "pangomm" => candidates.push("pangomm-1.4".to_string()),
        "pcre2" => candidates.push("libpcre2-8".to_string()),
        "zlib" => candidates.push("zlib".to_string()),
        _ => {}
    }

    candidates.sort();
    candidates.dedup();
    candidates
}

fn host_file_candidates(name: &str) -> Vec<String> {
    let library_name = name.trim_start_matches("lib");

    let mut candidates = vec![
        format!("/usr/bin/{}", name),
        format!("/usr/sbin/{}", name),
        format!("/bin/{}", name),
        format!("/sbin/{}", name),
        format!("/usr/lib/lib{}.so", library_name),
        format!("/usr/lib64/lib{}.so", library_name),
        format!("/lib/lib{}.so", library_name),
        format!("/lib64/lib{}.so", library_name),
    ];

    let command_names: &[&str] = match name {
        "binutils" => &["as", "ld", "objdump"],
        "coreutils" => &["ls", "cp", "mv", "mkdir"],
        "diffutils" => &["diff", "cmp"],
        "findutils" => &["find", "xargs"],
        "gawk" => &["awk", "gawk"],
        "gzip" => &["gzip", "gunzip"],
        "pkgconf" => &["pkg-config", "pkgconf"],
        "shadow" => &["useradd", "groupadd"],
        "util-linux" => &["mount", "umount", "lsblk"],
        _ => &[],
    };

    for command in command_names {
        candidates.push(format!("/usr/bin/{}", command));
        candidates.push(format!("/usr/sbin/{}", command));
        candidates.push(format!("/bin/{}", command));
        candidates.push(format!("/sbin/{}", command));
    }

    candidates
}

pub fn install_package(
    package: &Package,
    flags: &[String],
    install_reason: &str,
) -> Result<(), String> {
    build_package(package, flags)?;

    println!("Installing {} {}", package.name, package.version);

    let files = install_staged_files(package)?;
    write_install_record(package, &files, install_reason)?;

    println!("Installed: {} ({} files)", package.name, files.len());

    Ok(())
}

pub fn is_installed_same_version(package: &Package) -> Result<bool, String> {
    let Some(record) = load_install_record(&package.name)? else {
        return Ok(false);
    };

    Ok(record.version == package.version)
}

pub fn install_record_path_for_name(name: &str) -> String {
    format!("{}/{}.toml", install_db_dir(), name)
}

pub fn load_install_record(name: &str) -> Result<Option<InstallRecord>, String> {
    let path = install_record_path_for_name(name);

    if fs::metadata(&path).is_err() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("Failed to read install record {}: {}", path, error))?;

    let record = toml::from_str(&contents)
        .map_err(|error| format!("Failed to parse install record {}: {}", path, error))?;

    Ok(Some(record))
}

pub fn load_installed_packages() -> Result<Vec<InstallRecord>, String> {
    let mut records = Vec::new();

    if fs::metadata(install_db_dir()).is_err() {
        return Ok(records);
    }

    let entries = fs::read_dir(install_db_dir())
        .map_err(|error| format!("Failed to read install database: {}", error))?;

    for entry in entries {
        let entry =
            entry.map_err(|error| format!("Failed to read install database entry: {}", error))?;
        let path = entry.path();

        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }

        let contents = fs::read_to_string(&path)
            .map_err(|error| format!("Failed to read {}: {}", path.display(), error))?;

        let record = toml::from_str(&contents)
            .map_err(|error| format!("Failed to parse {}: {}", path.display(), error))?;

        records.push(record);
    }

    records.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(records)
}

pub fn find_file_owner(path: &str) -> Result<Option<String>, String> {
    for record in load_installed_packages()? {
        if record.files.iter().any(|file| file == path) {
            return Ok(Some(record.name));
        }
    }

    Ok(None)
}

pub fn check_file_conflicts(package: &Package, files: &[String]) -> Result<(), String> {
    for file in files {
        let Some(owner) = find_file_owner(file)? else {
            continue;
        };

        if owner != package.name {
            return Err(format!(
                "File conflict: {} is already owned by {}",
                file, owner
            ));
        }
    }

    Ok(())
}

pub fn uninstall_package(name: &str, dry_run: bool) -> Result<(), String> {
    let Some(record) = load_install_record(name)? else {
        return Err(format!("Package is not installed: {}", name));
    };

    let mut removed = 0;

    for file in record.files.iter().rev() {
        match fs::symlink_metadata(file) {
            Ok(metadata) => {
                if metadata.is_file() || metadata.file_type().is_symlink() {
                    if dry_run {
                        println!("Would remove: {}", file);
                    } else {
                        fs::remove_file(file)
                            .map_err(|error| format!("Failed to remove {}: {}", file, error))?;

                        cleanup_empty_parent_dirs(file);
                    }

                    removed += 1;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("Warning: already missing: {}", file);
            }
            Err(error) => {
                return Err(format!("Failed to inspect {}: {}", file, error));
            }
        }
    }

    let record_path = install_record_path_for_name(name);

    if dry_run {
        println!("Would remove install record: {}", record_path);
        println!("Dry run: {} ({} files would be removed)", name, removed);
    } else {
        fs::remove_file(&record_path).map_err(|error| {
            format!("Failed to remove install record {}: {}", record_path, error)
        })?;

        println!("Uninstalled {} ({} files removed)", name, removed);
    }

    Ok(())
}

fn cleanup_empty_parent_dirs(file: &str) {
    let Some(mut dir) = std::path::Path::new(file).parent() else {
        return;
    };

    while should_cleanup_dir(dir) {
        match fs::remove_dir(dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => return,
            Err(error) => {
                eprintln!(
                    "Warning: failed to remove empty directory {}: {}",
                    dir.display(),
                    error
                );
                return;
            }
        }

        let Some(parent) = dir.parent() else {
            return;
        };

        dir = parent;
    }
}

fn should_cleanup_dir(dir: &std::path::Path) -> bool {
    let protected = [
        "/",
        "/bin",
        "/etc",
        "/lib",
        "/lib64",
        "/sbin",
        "/usr",
        "/usr/bin",
        "/usr/etc",
        "/usr/include",
        "/usr/lib",
        "/usr/lib64",
        "/usr/sbin",
        "/usr/share",
        "/var",
        "/var/lib",
    ];

    let dir_string = dir.to_string_lossy();

    !protected.iter().any(|path| dir_string == *path)
}
