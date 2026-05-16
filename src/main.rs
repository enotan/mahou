use std::env;
use std::fs;
use std::collections::HashSet;
use serde::Deserialize;
use sha2::{Sha256, Digest};
use std::process::Command;
use std::os::unix::fs as unix_fs;

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
            let packages = match load_packages("repo") {
                Ok(packages) => packages,
                Err(message) => {
                    eprint!("error: {}", message);
                    return;
                }
            };

            for package in packages {
                if package.name.contains(query) {
                    println!("{} {} - {}", package.name, package.version, package.description);
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
            let packages = load_repo_or_exit();

            match resolve_package_order(&packages, name) {
                Ok(order) => {
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
                Err(message) => {
                    eprintln!("Error: {}", message);
                }
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
}

fn load_packages(repo_path: &str) -> Result<Vec<Package>, String> {
    let mut packages = Vec::new();

    let entries = fs::read_dir(repo_path).map_err(|error| format!("Failed to read repo directory '{}': {}", repo_path, error))?;

    for entry in entries {
        let entry = entry.map_err(|error| format!("Failed to read directory entry: {}", error))?;
        let path = entry.path();

        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }

        let contents = fs::read_to_string(&path).map_err(|error| format!("Failed to read package file '{}': {}", path.display(), error))?;
        let package = toml::from_str(&contents).map_err(|error| format!("Failed to parse package file '{}': {}", path.display(), error))?;

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
    match load_packages("repo") {
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
    
    let client = reqwest::blocking::Client::builder()
        .user_agent("mahou/0.1")
        .build()
        .map_err(|error| format!("Failed to create HTTP client: {}", error))?;

    let response = client
        .get(&package.source)
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
    }
    else {
        Err(format!("Expected source directory '{}' not found after extraction", source_dir))
    }
}

fn run_build_step(package: &Package, step: &str) -> Result<(), String> {
    let source_dir = format!("{}/{}", build_dir(), package.source_dir);
    let destdir = stage_dir(package);

    fs::create_dir_all(&destdir)
        .map_err(|error| format!("Failed to create stage directory '{}': {}", destdir, error))?;

    println!("Running build step for {}: {}", package.name, step);

    let status = Command::new("sh")
        .arg("-c")
        .arg(step)
        .current_dir(&source_dir)
        .env("MAHOU_DESTDIR", &destdir)
        .status()
        .map_err(|error| format!("Failed to run build step '{}': {}", step, error))?;

    if !status.success() {
        return Err(format!("Build step failed for {}: {}", package.name, step));
    }

    Ok(())
}

fn build_package(package: &Package) -> Result<(), String> {
    if is_built(package) {
        println!("Already built: {}", package.name);
        return Ok(());
    }
    
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
    let contents = fs::read(path)
        .map_err(|error| format!("Failed to read {}: {}", path, error))?;

    let mut hasher = Sha256::new();
    hasher.update(contents);

    Ok(hex::encode(hasher.finalize()))
}

fn build_marker_path(package: &Package) -> String {
    format!("{}/.mahou-built", stage_dir(package))
}

fn is_built(package: &Package) -> bool {
    fs::metadata(build_marker_path(package)).is_ok()
}

fn mark_built(package: &Package) -> Result<(), String> {
    let marker_path = build_marker_path(package);

    fs::write(
        &marker_path,
        format!("{} {}\n", package.name, package.version)
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

fn collect_files_recursive(base: &str, current: &str, files: &mut Vec<String>) -> Result<(), String> {
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
            fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to create directory {}: {}", parent.display(), error))?;
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
        }
        else {
            fs::copy(&source, &target)
                .map_err(|error| format!("Failed to copy {} to {}: {}", source, target, error))?;
        }
    }

    Ok(files)

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

fn cache_dir() -> &'static str {
    "/var/cache/mahou"
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
