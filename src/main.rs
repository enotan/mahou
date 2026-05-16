use std::env;
use std::fs;
use std::collections::HashSet;
use std::mem;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Package {
    name: String,
    version: String,
    description: String,
    source: String,
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

            match find_packages(&packages, name) {
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

            match find_packages(&packages, name) {
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
                        let Some(package) = find_packages(&packages, &package_name) else {
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
        "build" => {
            if args.len() < 3 {
                eprintln!("Missing package name");
                return;
            }

            let name = &args[2];
            let packages = load_repo_or_exit();

            match resolve_package_order(&packages, name) {
                Ok(order) => {
                    println!("Build plan:");

                    for package_name in order {
                        let Some(package) = find_packages(&packages, &package_name) else {
                            eprintln!("Package vanished from repo: {}", package_name);
                            return;
                        };

                        println!();
                        println!("{} {}", package.name, package.version);

                        if package.build_steps.is_empty() {
                            println!("No build steps...");
                        } else {
                            for step in &package.build_steps {
                                println!(" - {}", step);
                            }
                        }
                    }
                }
                Err(message) => {
                    eprintln!("{}", message);
                }
            }

        }
        "install" => {
            println!("Installing package...");
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

fn find_packages<'a>(packages: &'a [Package], name: &str) -> Option<&'a Package> {
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
        match find_packages(packages, dep_name) {
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

    let Some(package) = find_packages(packages, name) else {
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
    fs::create_dir_all("distfiles")
        .map_err(|error| format!("Failed to create distfiles directory: {}", error))?;

    let filename = package
        .source
        .rsplit('/')
        .next()
        .ok_or_else(|| format!("Invalid source URL for {}", package.name))?;

    if filename.is_empty() {
        return Err(format!("Invalid source URL for {}", package.name));
    }

    let output_path = format!("distfiles/{}", filename);

    if fs::metadata(&output_path).is_ok() {
        println!("Already fetched: {}", output_path);
        return Ok(());
    }

    println!("Fetching {} from {}", package.name, package.source);

    let response = reqwest::blocking::get(&package.source)
        .map_err(|error| format!("Failed to download {}: {}", package.name, error))?
        .error_for_status()
        .map_err(|error| format!("Download failed for {}: {}", package.source, error))?;

    let bytes = response
        .bytes()
        .map_err(|error| format!("Failed to read response for {}: {}", package.name, error))?;

    fs::write(&output_path, &bytes)
        .map_err(|error| format!("Failed to save {}: {}", output_path, error))?;

    println!("Saved to {}", output_path);

    Ok(())
}
