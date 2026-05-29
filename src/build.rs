use crate::config::{add_auth_header, build_dir, distfiles_dir, http_client, stage_dir};

use crate::package::{InstallRecord, Package};

use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::os::unix::fs as unix_fs;
use std::process::Command;

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

    let bytes = response
        .bytes()
        .map_err(|error| format!("Failed to read response for {}: {}", package.name, error))?;

    fs::write(&partial_path, &bytes)
        .map_err(|error| format!("Failed to save {}: {}", partial_path, error))?;

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
    let source_dir = format!("{}/{}", build_dir(), package.source_dir);

    if fs::metadata(&source_dir).is_ok() {
        println!("Already extracted: {}", source_dir);
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

    if fs::metadata(&source_dir).is_ok() {
        println!("Extracted: {}", source_dir);
        Ok(())
    } else {
        Err(format!(
            "Expected source directory '{}' not found after extraction",
            source_dir
        ))
    }
}

fn run_build_step(package: &Package, step: &str) -> Result<(), String> {
    let source_dir = format!("{}/{}", build_dir(), package.source_dir);
    let destdir = stage_dir(package);

    fs::create_dir_all(&destdir)
        .map_err(|error| format!("Failed to create stage directory '{}': {}", destdir, error))?;

    println!("Running build step for {}: {}", package.name, step);

    let pkg_config_path = build_env_path(
        "PKG_CONFIG_PATH",
        &[
            "/usr/lib/pkgconfig",
            "/usr/lib64/pkgconfig",
            "/usr/share/pkgconfig",
        ],
    );

    let library_path = build_env_path("LIBRARY_PATH", &["/usr/lib", "/usr/lib64"]);

    let ld_library_path = build_env_path("LD_LIBRARY_PATH", &["/usr/lib", "/usr/lib64"]);

    let status = Command::new("sh")
        .arg("-c")
        .arg(step)
        .current_dir(&source_dir)
        .env("MAHOU_DESTDIR", &destdir)
        .env("PKG_CONFIG_PATH", pkg_config_path)
        .env("LIBRARY_PATH", library_path)
        .env("LD_LIBRARY_PATH", ld_library_path)
        .env("CMAKE_PREFIX_PATH", "/usr")
        .status()
        .map_err(|error| format!("Failed to run build step '{}': '{}'", step, error))?;

    if !status.success() {
        return Err(format!("Build step failed for {}: {}", package.name, step));
    }

    Ok(())
}

fn build_env_path(name: &str, defaults: &[&str]) -> String {
    let mut paths: Vec<String> = defaults.iter().map(|path| path.to_string()).collect();

    if let Ok(existing) = env::var(name) {
        for path in existing.split(':') {
            if !path.is_empty() && !paths.iter().any(|existing| existing == path) {
                paths.push(path.to_string());
            }
        }
    }

    paths.join(":")
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

pub fn clean_stage(package: &Package) -> Result<(), String> {
    let path = stage_dir(package);

    if fs::metadata(&path).is_ok() {
        fs::remove_dir_all(&path)
            .map_err(|error| format!("Failed to remove stage directory {}: {}", path, error))?;
    }

    Ok(())
}

fn collect_files(root: &str) -> Result<Vec<String>, String> {
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

fn write_install_record(package: &Package, files: &[String]) -> Result<(), String> {
    fs::create_dir_all(install_db_dir())
        .map_err(|error| format!("Failed to create install database: {}", error))?;

    let path = install_record_path(package);

    let mut contents = String::new();
    contents.push_str(&format!("name = \"{}\"\n", package.name));
    contents.push_str(&format!("version = \"{}\"\n", package.version));
    contents.push_str("files = [\n");

    for file in files {
        contents.push_str(&format!("    \"{}\",\n", file));
    }

    contents.push_str("]\n");

    fs::write(&path, contents)
        .map_err(|error| format!("Failed to write install record {}: {}", path, error))?;

    Ok(())
}

pub fn install_package(package: &Package, flags: &[String]) -> Result<(), String> {
    build_package(package, flags)?;

    println!("Installing {} {}", package.name, package.version);

    let files = install_staged_files(package)?;
    write_install_record(package, &files)?;

    println!("Installed: {} ({} files)", package.name, files.len());

    Ok(())
}

pub fn install_db_dir() -> &'static str {
    "/var/lib/mahou/installed"
}

fn install_record_path(package: &Package) -> String {
    format!("{}/{}.toml", install_db_dir(), package.name)
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
