use regex::Regex;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs as unix_fs;
use std::process::Command;

#[derive(Debug, Deserialize)]
struct Package {
    name: String,
    version: String,
    description: String,
    source: String,
    sha256: String,
    source_dir: String,
    deps: Vec<String>,
    build_steps: Vec<String>,
    update: Option<UpdateInfo>,
}

#[derive(Debug, Deserialize)]
struct UpdateInfo {
    check_url: String,
    version_pattern: String,
    source_template: String,
    source_dir_template: String,
}

#[derive(Debug, Deserialize)]
struct InstallRecord {
    name: String,
    version: String,
    files: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct MahouConfig {
    github_token: Option<String>,
    recipe_repo_url: Option<String>,
    recipe_repo_path: Option<String>,
    cache_dir: Option<String>,
}

struct RecipeUpdate {
    version: String,
    source: String,
    sha256: String,
    source_dir: String,
}
fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_help();
        return;
    }

    let command = &args[1];

    match command.as_str() {
        "search" => {
            if args.len() < 3 {
                eprint!("Please provide a package name to search for.");
                return;
            }

            let query = &args[2];
            let repo_path = recipe_repo_path();

            let packages = match load_packages(&repo_path) {
                Ok(packages) => packages,
                Err(message) => {
                    eprint!("error: {}", message);
                    return;
                }
            };

            for package in packages {
                if package.name.contains(query) {
                    println!(
                        "{} {} - {}",
                        package.name, package.version, package.description
                    );
                }
            }
        }
        "info" => {
            if args.len() < 3 {
                eprintln!("Missing package name");
                return;
            }

            let name = &args[2];
            let packages = load_repo_or_exit();

            match find_package(&packages, name) {
                Some(package) => {
                    println!("Name: {}", package.name);
                    println!("Version: {}", package.version);
                    println!("Description: {}", package.description);
                    println!("Source: {}", package.source);

                    if package.deps.is_empty() {
                        println!("Dependencies: None");
                    } else {
                        println!("Dependencies: {}", package.deps.join(", "));
                    }

                    if package.build_steps.is_empty() {
                        println!("Build: None");
                    } else {
                        println!("Build:");
                        for step in &package.build_steps {
                            println!(" - {}", step);
                        }
                    }
                }
                None => {
                    eprintln!("Package not found: {}", name);
                }
            }
        }
        "deps" => {
            if args.len() < 3 {
                eprintln!("Missing package name");
                return;
            }

            let name = &args[2];
            let packages = load_repo_or_exit();

            match find_package(&packages, name) {
                Some(package) => {
                    print_deps(&packages, package, 0);
                }
                None => {
                    eprintln!("Package not found: {}", name);
                }
            }
        }
        "resolve" => {
            if args.len() < 3 {
                eprintln!("Missing package name");
                return;
            }

            let name = &args[2];
            let packages = load_repo_or_exit();

            match resolve_package_order(&packages, name) {
                Ok(order) => {
                    for package_name in order {
                        println!("{}", package_name);
                    }
                }
                Err(message) => {
                    eprint!("{}", message);
                }
            }
        }
        "fetch" => {
            if args.len() < 3 {
                eprintln!("Missing package name");
                return;
            }

            let name = &args[2];
            let packages = load_repo_or_exit();

            match resolve_package_order(&packages, name) {
                Ok(order) => {
                    for package_name in order {
                        let Some(package) = find_package(&packages, &package_name) else {
                            eprintln!("error: package vanished from repo: {}", package_name);
                            return;
                        };

                        if let Err(message) = fetch_package(package) {
                            eprintln!("Failed to fetch package '{}': {}", package.name, message);
                            return;
                        }
                    }
                }
                Err(message) => {
                    eprintln!("{}", message);
                }
            }
        }
        "verify" => {
            if args.len() < 3 {
                eprintln!("Missing package name");
                return;
            }

            let name = &args[2];
            let packages = load_repo_or_exit();

            match resolve_package_order(&packages, name) {
                Ok(order) => {
                    for package_name in order {
                        let Some(package) = find_package(&packages, &package_name) else {
                            eprintln!("Error: package vanished from repo: {}", package_name);
                            return;
                        };

                        if let Err(message) = verify_package(package) {
                            eprintln!("Failed to verify package '{}': {}", package.name, message);
                            return;
                        }
                    }
                }
                Err(message) => {
                    eprintln!("Error: {}", message);
                }
            }
        }
        "extract" => {
            if args.len() < 3 {
                eprintln!("Missing package name");
                return;
            }

            let name = &args[2];
            let packages = load_repo_or_exit();

            match resolve_package_order(&packages, name) {
                Ok(order) => {
                    for package_name in order {
                        let Some(package) = find_package(&packages, &package_name) else {
                            eprintln!("Error: Package vanished from repo: {}", package_name);
                            return;
                        };

                        if let Err(message) = fetch_package(package) {
                            eprintln!("Error: {}", message);
                            return;
                        }

                        if let Err(message) = extract_package(package) {
                            eprintln!("Error: {}", message);
                            return;
                        }
                    }
                }
                Err(message) => {
                    eprintln!("Error: {}", message);
                }
            }
        }
        "rebuild" => {
            if args.len() < 3 {
                eprintln!("Missing package name");
                return;
            }

            let name = &args[2];
            let packages = load_repo_or_exit();

            match resolve_package_order(&packages, name) {
                Ok(order) => {
                    for package_name in order {
                        let Some(package) = find_package(&packages, &package_name) else {
                            eprintln!("Error: Package vanished from repo: {}", package_name);
                            return;
                        };

                        if let Err(message) = clean_stage(package) {
                            eprintln!("Error: {}", message);
                            return;
                        }

                        if let Err(message) = build_package(package) {
                            eprintln!("Error: {}", message);
                            return;
                        }
                    }
                }
                Err(message) => {
                    eprintln!("Error: {}", message);
                }
            }
        }
        "build" => {
            if args.len() < 3 {
                eprintln!("Missing package name");
                return;
            }

            let name = &args[2];
            let packages = load_repo_or_exit();

            match resolve_package_order(&packages, name) {
                Ok(order) => {
                    for package_name in order {
                        let Some(package) = find_package(&packages, &package_name) else {
                            eprintln!("Error: Package vanished from repo: {}", package_name);
                            return;
                        };

                        if let Err(message) = build_package(package) {
                            eprintln!("Error: {}", message);
                            return;
                        }
                    }
                }
                Err(message) => {
                    eprintln!("{}", message);
                }
            }
        }
        "install" => {
            if args.len() < 3 {
                eprintln!("Missing package name");
                return;
            }

            let name = &args[2];
            let mut packages = load_repo_or_exit();

            let order = match resolve_package_order(&packages, name) {
                Ok(order) => order,
                Err(message) => {
                    eprintln!("Error: {}", message);
                    return;
                }
            };

            match refresh_recipes_for_order(&packages, &order) {
                Ok(true) => {
                    packages = load_repo_or_exit();
                }
                Ok(false) => {}
                Err(message) => {
                    eprintln!("Error: {}", message);
                    return;
                }
            }

            let order = match resolve_package_order(&packages, name) {
                Ok(order) => order,
                Err(message) => {
                    eprintln!("Error: {}", message);
                    return;
                }
            };

            for package_name in order {
                let Some(package) = find_package(&packages, &package_name) else {
                    eprintln!("Error: Package vanished from repo: {}", package_name);
                    return;
                };

                if let Err(message) = install_package(package) {
                    eprintln!("Error: {}", message);
                    return;
                }
            }
        }
        "list" => match load_installed_packages() {
            Ok(records) => {
                if records.is_empty() {
                    println!("No packages installed");
                    return;
                }

                for record in records {
                    println!("{} {}", record.name, record.version);
                }
            }

            Err(message) => {
                eprintln!("Error: {}", message);
            }
        },
        "files" => {
            if args.len() < 3 {
                eprintln!("missing package name");
                return;
            }

            let name = &args[2];

            match load_install_record(name) {
                Ok(Some(record)) => {
                    for file in record.files {
                        println!("{}", file);
                    }
                }
                Ok(None) => {
                    eprintln!("Package is not installed: {}", name);
                }
                Err(message) => {
                    eprintln!("Error: {}", message);
                }
            }
        }
        "outdated" => {
            let packages = load_repo_or_exit();

            match load_installed_packages() {
                Ok(records) => {
                    let mut found = false;

                    for record in records {
                        let Some(package) = find_package(&packages, &record.name) else {
                            continue;
                        };

                        if record.version != package.version {
                            println!("{} {} -> {}", record.name, record.version, package.version);
                            found = true;
                        }
                    }

                    if !found {
                        println!("All packages are up to date");
                    }
                }
                Err(message) => {
                    eprintln!("Error: {}", message);
                }
            }
        }
        "update-check" => {
            if args.len() < 3 {
                eprintln!("Missing package name");
                return;
            }

            let name = &args[2];
            let packages = load_repo_or_exit();

            match find_package(&packages, name) {
                Some(package) => match check_for_update(package) {
                    Ok(Some(latest)) => {
                        if latest == package.version {
                            println!("{} is up to date ({})", package.name, package.version);
                        } else {
                            println!("{} {} -> {}", package.name, package.version, latest);

                            match plan_recipe_update(package, &latest) {
                                Ok(plan) => {
                                    println!("New source: {}", plan.source);
                                    println!("New source dir: {}", plan.source_dir);
                                    println!("New sha256: {}", plan.sha256);
                                }
                                Err(message) => {
                                    eprintln!("Failed to plan recipe update: {}", message);
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        println!("{} has no update metadata", package.name);
                    }
                    Err(message) => {
                        eprintln!("Error: {}", message);
                    }
                },
                None => {
                    eprintln!("Package not found: {}", name);
                }
            }
        }
        "update-recipe" => {
            if args.len() < 3 {
                eprintln!("Missing package name");
                return;
            }

            let name = &args[2];
            let packages = load_repo_or_exit();

            match find_package(&packages, name) {
                Some(package) => match check_for_update(package) {
                    Ok(Some(latest)) => {
                        if latest == package.version {
                            println!(
                                "{} is already up to date ({})",
                                package.name, package.version
                            );
                            return;
                        }

                        match plan_recipe_update(package, &latest) {
                            Ok(plan) => {
                                if let Err(message) = write_recipe_update(package, &plan) {
                                    eprintln!("Error: {}", message);
                                    return;
                                }

                                println!("Updated recipe: {}", recipe_path(&package.name));
                                println!(
                                    "{} {} -> {}",
                                    package.name, package.version, plan.version
                                );
                            }
                            Err(message) => {
                                eprintln!("Error: {}", message);
                            }
                        }
                    }
                    Ok(None) => {
                        println!("{} has no update metadata", package.name);
                    }
                    Err(message) => {
                        eprintln!("Error: {}", message);
                    }
                },
                None => {
                    eprintln!("Package not found: {}", name);
                }
            }
        }
        "repo-path" => {
            println!("{}", recipe_repo_path());
        }
        "sync" => {
            if let Err(message) = sync_recipe_repo() {
                eprintln!("Error: {}", message);
                return;
            }

            if let Err(message) = sync_upstream_recipes() {
                eprintln!("Error: {}", message);
            }
        }
        "upgrade" => {
            if let Err(message) = upgrade_installed_packages() {
                eprintln!("Error: {}", message);
            }
        }
        "init-config" => {
            if let Err(message) = init_config() {
                eprintln!("Error: {}", message);
            }
        }
        _ => {
            eprintln!("Unknown command: {}", command);
            print_help();
        }
    }
}

fn print_help() {
    println!("mahou - A Witch's favourite package manager");
    println!();
    println!("Usage:");
    println!("  mahou search <name>");
    println!("  mahou info <name>");
    println!("  mahou build <name>");
    println!("  mahou install <name>");
    println!("  mahou deps <name>");
    println!("  mahou resolve <name>");
    println!("  mahou fetch <name>");
    println!("  mahou verify <name>");
    println!("  mahou extract <name>");
    println!("  mahou rebuild <name>");
    println!("  mahou help");
    println!("  mahou files <name>");
    println!("  mahou outdated");
    println!("  mahou update-check <name>");
    println!("  mahou update-recipe <name>");
    println!("  mahou repo-path");
    println!("  mahou sync");
    println!("  mahou upgrade");
    println!("  mahou init-config");
}

fn load_packages(repo_path: &str) -> Result<Vec<Package>, String> {
    let mut packages = Vec::new();

    let entries = fs::read_dir(repo_path)
        .map_err(|error| format!("Failed to read repo directory '{}': {}", repo_path, error))?;

    for entry in entries {
        let entry = entry.map_err(|error| format!("Failed to read directory entry: {}", error))?;
        let path = entry.path();

        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }

        let contents = fs::read_to_string(&path).map_err(|error| {
            format!(
                "Failed to read package file '{}': {}",
                path.display(),
                error
            )
        })?;
        let package = toml::from_str(&contents).map_err(|error| {
            format!(
                "Failed to parse package file '{}': {}",
                path.display(),
                error
            )
        })?;

        packages.push(package);
    }

    Ok(packages)
}

fn find_package<'a>(packages: &'a [Package], name: &str) -> Option<&'a Package> {
    packages.iter().find(|package| package.name == name)
}

fn print_deps(packages: &[Package], package: &Package, depth: usize) {
    let indent = "  ".repeat(depth);

    if depth == 0 {
        println!("{}", package.name);
    } else {
        println!("{}└── {}", indent, package.name);
    }

    for dep_name in &package.deps {
        match find_package(packages, dep_name) {
            Some(dep) => print_deps(packages, dep, depth + 1),
            None => println!("{} └── {} (missing)", indent, dep_name),
        }
    }
}

fn resolve_package_order(packages: &[Package], name: &str) -> Result<Vec<String>, String> {
    let mut visited = HashSet::new();
    let mut order = Vec::new();

    resolve_package(packages, name, &mut visited, &mut order)?;

    Ok(order)
}

fn resolve_package(
    packages: &[Package],
    name: &str,
    visited: &mut HashSet<String>,
    order: &mut Vec<String>,
) -> Result<(), String> {
    if visited.contains(name) {
        return Ok(());
    }

    let Some(package) = find_package(packages, name) else {
        return Err(format!("Missing package: {}", name));
    };

    visited.insert(name.to_string());

    for dep_name in &package.deps {
        resolve_package(packages, dep_name, visited, order)?;
    }

    order.push(package.name.clone());

    Ok(())
}

fn load_repo_or_exit() -> Vec<Package> {
    let repo_path = recipe_repo_path();

    match load_packages(&repo_path) {
        Ok(packages) => packages,
        Err(message) => {
            eprintln!("{}", message);
            std::process::exit(1);
        }
    }
}

fn fetch_package(package: &Package) -> Result<(), String> {
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

fn verify_package(package: &Package) -> Result<(), String> {
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

fn extract_package(package: &Package) -> Result<(), String> {
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

fn build_package(package: &Package) -> Result<(), String> {
    if is_built(package) {
        println!("Already built: {}", package.name);
        return Ok(());
    }

    clean_stage(package)?;

    extract_package(package)?;

    println!("Building {} {}", package.name, package.version);

    for step in &package.build_steps {
        run_build_step(package, step)?;
    }

    mark_built(package)?;

    println!("Built: {}", package.name);

    Ok(())
}

fn sha256_file(path: &str) -> Result<String, String> {
    let contents = fs::read(path).map_err(|error| format!("Failed to read {}: {}", path, error))?;

    Ok(sha256_bytes(&contents))
}

fn sha256_bytes(contents: &[u8]) -> String {
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

fn clean_stage(package: &Package) -> Result<(), String> {
    let path = stage_dir(package);

    if fs::metadata(&path).is_ok() {
        fs::remove_dir_all(&path)
            .map_err(|error| format!("Failed to remove stage directory {}: {}", path, error))?;
    }

    Ok(())
}

fn install_db_dir() -> &'static str {
    "/var/lib/mahou/installed"
}

fn install_record_path(package: &Package) -> String {
    format!("{}/{}.toml", install_db_dir(), package.name)
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

fn install_staged_files(package: &Package) -> Result<Vec<String>, String> {
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

fn install_package(package: &Package) -> Result<(), String> {
    build_package(package)?;

    println!("Installing {} {}", package.name, package.version);

    let files = install_staged_files(package)?;
    write_install_record(package, &files)?;

    println!("Installed: {} ({} files)", package.name, files.len());

    Ok(())
}

fn cache_dir() -> String {
    load_config()
        .cache_dir
        .unwrap_or_else(|| default_cache_dir().to_string())
}

fn distfiles_dir() -> String {
    format!("{}/distfiles", cache_dir())
}

fn build_dir() -> String {
    format!("{}/build", cache_dir())
}

fn stage_root() -> String {
    format!("{}/stage", cache_dir())
}

fn stage_dir(package: &Package) -> String {
    format!("{}/{}", stage_root(), package.name)
}

fn install_record_path_for_name(name: &str) -> String {
    format!("{}/{}.toml", install_db_dir(), name)
}

fn load_install_record(name: &str) -> Result<Option<InstallRecord>, String> {
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

fn load_installed_packages() -> Result<Vec<InstallRecord>, String> {
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

fn check_for_update(package: &Package) -> Result<Option<String>, String> {
    let Some(update) = &package.update else {
        return Ok(None);
    };

    let client = http_client()?;

    let body = add_auth_header(client.get(&update.check_url), &update.check_url)
        .send()
        .map_err(|error| format!("Failed to check {}: {}", update.check_url, error))?
        .error_for_status()
        .map_err(|error| format!("Update check failed for {}: {}", update.check_url, error))?
        .text()
        .map_err(|error| format!("Failed to read update response: {}", error))?;

    let regex = Regex::new(&update.version_pattern)
        .map_err(|error| format!("Invalid version pattern for {}: {}", package.name, error))?;

    let mut newest = package.version.clone();

    for captures in regex.captures_iter(&body) {
        let Some(version) = captures.get(1) else {
            continue;
        };

        let version = version.as_str();

        if compare_versions(version, &newest).is_gt() {
            newest = version.to_string();
        }
    }

    Ok(Some(newest))
}

fn version_parts(version: &str) -> Vec<u64> {
    version
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

fn compare_versions(left: &str, right: &str) -> Ordering {
    let left_parts = version_parts(left);
    let right_parts = version_parts(right);
    let max_len = left_parts.len().max(right_parts.len());

    for i in 0..max_len {
        let left_part = left_parts.get(i).copied().unwrap_or(0);
        let right_part = right_parts.get(i).copied().unwrap_or(0);

        match left_part.cmp(&right_part) {
            Ordering::Equal => {}
            ordering => return ordering,
        }
    }

    Ordering::Equal
}

fn render_update_template(template: &str, version: &str) -> String {
    template.replace("{version}", version)
}

fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    let client = http_client()?;

    let response = add_auth_header(client.get(url), url)
        .send()
        .map_err(|error| format!("Failed to download {}: {}", url, error))?
        .error_for_status()
        .map_err(|error| format!("Download failed for {}: {}", url, error))?;

    let bytes = response
        .bytes()
        .map_err(|error| format!("Failed to read response for {}: {}", url, error))?;

    Ok(bytes.to_vec())
}

fn recipe_path(name: &str) -> String {
    format!("{}/{}.toml", recipe_repo_path(), name)
}

fn plan_recipe_update(package: &Package, latest: &str) -> Result<RecipeUpdate, String> {
    let Some(update) = &package.update else {
        return Err(format!("{} has no update metadata", package.name));
    };

    let source = render_update_template(&update.source_template, latest);
    let source_dir = render_update_template(&update.source_dir_template, latest);
    let bytes = download_bytes(&source)?;
    let sha256 = sha256_bytes(&bytes);

    Ok(RecipeUpdate {
        version: latest.to_string(),
        source,
        sha256,
        source_dir,
    })
}

fn replace_toml_string_field(contents: &str, key: &str, value: &str) -> String {
    let mut result = String::new();
    let prefix = format!("{} = ", key);
    let replacement = format!("{} = \"{}\"", key, value);

    for line in contents.lines() {
        if line.starts_with(&prefix) {
            result.push_str(&replacement);
        } else {
            result.push_str(line);
        }

        result.push('\n');
    }

    result
}

fn write_recipe_update(package: &Package, plan: &RecipeUpdate) -> Result<(), String> {
    let path = recipe_path(&package.name);

    let contents =
        fs::read_to_string(&path).map_err(|error| format!("Failed to read {}: {}", path, error))?;

    let contents = replace_toml_string_field(&contents, "version", &plan.version);
    let contents = replace_toml_string_field(&contents, "source", &plan.source);
    let contents = replace_toml_string_field(&contents, "sha256", &plan.sha256);
    let contents = replace_toml_string_field(&contents, "source_dir", &plan.source_dir);

    fs::write(&path, contents).map_err(|error| format!("Failed to write {}: {}", path, error))?;

    Ok(())
}

fn update_recipe_if_needed(package: &Package) -> Result<bool, String> {
    let Some(_) = &package.update else {
        return Ok(false);
    };

    let Some(latest) = check_for_update(package)? else {
        return Ok(false);
    };

    if latest == package.version {
        return Ok(false);
    }

    let plan = plan_recipe_update(package, &latest)?;
    write_recipe_update(package, &plan)?;

    println!(
        "Updated recipe: {} {} -> {}",
        package.name, package.version, plan.version
    );

    Ok(true)
}

fn refresh_recipes_for_order(packages: &[Package], order: &[String]) -> Result<bool, String> {
    let mut changed = false;

    for package_name in order {
        let Some(package) = find_package(packages, package_name) else {
            return Err(format!("Package vanishe from repo: {}", package_name));
        };

        if update_recipe_if_needed(package)? {
            changed = true;
        }
    }

    Ok(changed)
}

fn recipe_repo_path() -> String {
    if fs::metadata("repo").is_ok() {
        "repo".to_string()
    } else {
        load_config()
            .recipe_repo_path
            .unwrap_or_else(|| default_recipe_repo_path().to_string())
    }
}

fn recipe_repo_url() -> String {
    load_config()
        .recipe_repo_url
        .unwrap_or_else(|| default_recipe_repo_url().to_string())
}

fn recipe_checkout_path() -> String {
    let recipe_path = load_config()
        .recipe_repo_path
        .unwrap_or_else(|| default_recipe_repo_path().to_string());

    let path = std::path::Path::new(&recipe_path);

    if path.file_name().and_then(|name| name.to_str()) == Some("repo") {
        return path.parent().unwrap_or(path).to_string_lossy().to_string();
    }

    recipe_path
}

fn sync_recipe_repo() -> Result<(), String> {
    let repo_path = recipe_checkout_path();

    if fs::metadata(&repo_path).is_ok() {
        println!("Syncing recipe repo: {}", repo_path);

        let status = Command::new("git")
            .arg("-C")
            .arg(&repo_path)
            .arg("pull")
            .arg("--ff-only")
            .status()
            .map_err(|error| format!("Failed to run git pull: {}", error))?;

        if !status.success() {
            return Err("git pull failed".to_string());
        }

        return Ok(());
    }

    println!("Cloning recipe repo into {}", repo_path);

    if let Some(parent) = std::path::Path::new(&repo_path).parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create repo directory: {}", error))?;
    }

    let status = Command::new("git")
        .arg("clone")
        .arg(recipe_repo_url())
        .arg(&repo_path)
        .status()
        .map_err(|error| format!("Failed to run git clone: {}", error))?;

    if !status.success() {
        return Err("Git clone failed".to_string());
    }

    Ok(())
}

fn sync_upstream_recipes() -> Result<(), String> {
    let packages = load_repo_or_exit();

    let mut checked = 0;
    let mut updated = 0;
    let mut failed = 0;

    for package in &packages {
        if package.update.is_none() {
            continue;
        }

        checked += 1;

        let latest = match check_for_update(package) {
            Ok(Some(latest)) => latest,
            Ok(None) => continue,
            Err(message) => {
                failed += 1;
                eprintln!("Warning: failed to check {}: {}", package.name, message);
                continue;
            }
        };

        if latest == package.version {
            println!("Up to date: {}", package.name);
            continue;
        }

        let plan = match plan_recipe_update(package, &latest) {
            Ok(plan) => plan,
            Err(message) => {
                failed += 1;
                eprintln!(
                    "Warning: failed to prepare update for {}: {}",
                    package.name, message
                );
                continue;
            }
        };

        if let Err(message) = write_recipe_update(package, &plan) {
            failed += 1;
            eprintln!(
                "Warning: failed to write update for {}: {}",
                package.name, message
            );
            continue;
        }

        updated += 1;
        println!(
            "Updated: {} {} -> {}",
            package.name, package.version, plan.version
        );
    }

    println!(
        "Sync complete: checked {}, updated {}, failed {}",
        checked, updated, failed
    );

    Ok(())
}

fn upgrade_installed_packages() -> Result<(), String> {
    sync_recipe_repo()?;
    sync_upstream_recipes()?;

    let installed = load_installed_packages()?;
    let packages = load_repo_or_exit();

    if installed.is_empty() {
        println!("No packages installed");
        return Ok(());
    }

    let mut upgraded = false;

    for record in installed {
        let Some(package) = find_package(&packages, &record.name) else {
            println!("Installed package has no recipe: {}", record.name);
            continue;
        };

        if package.version == record.version {
            println!("Up to date: {} {}", package.name, package.version);
            continue;
        }

        println!(
            "Upgrading {} {} -> {}",
            package.name, record.version, package.version
        );

        install_package(package)?;
        upgraded = true;
    }

    if !upgraded {
        println!("All installed packages are up to date");
    }

    Ok(())
}

fn load_config() -> MahouConfig {
    let path = env::var("MAHOU_CONFIG").unwrap_or_else(|_| "/etc/mahou/config.toml".to_string());

    let Ok(contents) = fs::read_to_string(&path) else {
        return MahouConfig::default();
    };

    match toml::from_str(&contents) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("Warning: failed to parse {}: {}", path, error);
            MahouConfig::default()
        }
    }
}

fn github_token() -> Option<String> {
    env::var("GITHUB_TOKEN")
        .ok()
        .or_else(|| load_config().github_token)
}

fn http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent("mahou/0.2")
        .build()
        .map_err(|error| format!("Failed to create HTTP client: {}", error))
}

fn add_auth_header(
    request: reqwest::blocking::RequestBuilder,
    url: &str,
) -> reqwest::blocking::RequestBuilder {
    if !url.contains("github.com") && !url.contains("api.github.com") {
        return request;
    }

    match github_token() {
        Some(token) => request.bearer_auth(token),
        None => request,
    }
}

fn init_config() -> Result<(), String> {
    let path = config_path();

    if fs::metadata(&path).is_ok() {
        return Err(format!("Config already exists: {}", path));
    }

    if let Some(parent) = std::path::Path::new(&path).parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create config directory: {}", error))?;
    }

    print!("GitHub token (Optional, press Enter to skip): ");
    io::stdout()
        .flush()
        .map_err(|error| format!("Failed to flush stdout: {}", error))?;

    let mut token = String::new();
    io::stdin()
        .read_line(&mut token)
        .map_err(|error| format!("Failed to read token: {}", error))?;

    let token = token.trim();

    let token_line = if token.is_empty() {
        "# github_token = \"ghp_your_token_here\"".to_string()
    } else {
        format!("github_token = \"{}\"", token)
    };

    let contents = format!(
        r#"# Mahou configuration
# The GitHub token is optional, but it helps avoid rate limits.

{}

recipe_repo_url = "{}"
recipe_repo_path = "{}"
cache_dir = "{}"
"#,
        token_line,
        default_recipe_repo_url(),
        default_recipe_repo_path(),
        default_cache_dir(),
    );

    fs::write(&path, contents).map_err(|error| format!("Failed to write {}: {}", path, error))?;

    println!("Created config: {}", path);

    Ok(())
}

fn default_recipe_repo_url() -> &'static str {
    "https://github.com/enotan/mahou-recipes.git"
}

fn default_recipe_repo_path() -> &'static str {
    "/var/lib/mahou/repos/main/repo"
}

fn default_cache_dir() -> &'static str {
    "/var/cache/mahou"
}

fn config_path() -> String {
    env::var("MAHOU_CONFIG").unwrap_or_else(|_| "/etc/mahou/config.toml".to_string())
}
