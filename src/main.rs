use std::collections::HashSet;
use std::env;

mod config;
mod package;
mod repo;
mod build;
mod features;
mod update;
use config::*;
use package::{Package};
use repo::*;
use build::*;
use features::*;
use update::*;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_help();
        return;
    }

    let command = &args[1];

    //commands available for a user to use
    match command.as_str() {
        "search" => {
            if args.len() < 3 {
                eprint!("Please provide a package name to search for.");
                return;
            }
            
            //searches through the entire repo using the repo path

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

            //displays basic info about the package

            let name = &args[2];
            let packages = load_repo_or_exit();
            let feature_flags = active_feature_flags(&args[3..]);

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

                    print_features(package, &feature_flags);
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

            //lists a package's dependencies

            let name = &args[2];
            let packages = load_repo_or_exit();
            let feature_flags = active_feature_flags(&[]);

            match find_package(&packages, name) {
                Some(package) => {
                    print_deps(&packages, package, &feature_flags, 0);
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

            //shows the flattened dependency order - user probably will never use this

            let name = &args[2];
            let packages = load_repo_or_exit();
            let feature_flags = active_feature_flags(&[]);

            match resolve_package_order(&packages, name, &feature_flags) {
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

            //downloads the source arhive - user will probably never use this

            let name = &args[2];
            let packages = load_repo_or_exit();
            let feature_flags = active_feature_flags(&[]);

            match resolve_package_order(&packages, name, &feature_flags) {
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

            //checks the archive sha256 to the source - user will probably never use this

            let name = &args[2];
            let packages = load_repo_or_exit();
            let feature_flags = active_feature_flags(&[]);

            match resolve_package_order(&packages, name, &feature_flags) {
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

            //extracts the source archive - user will probably never use this

            let name = &args[2];
            let packages = load_repo_or_exit();
            let feature_flags = active_feature_flags(&[]);

            match resolve_package_order(&packages, name, &feature_flags) {
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

            //forces a clean rebuild - user will probably never use

            let name = &args[2];
            let packages = load_repo_or_exit();
            let feature_flags = active_feature_flags(&[]);

            match resolve_package_order(&packages, name, &feature_flags) {
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

                        if let Err(message) = build_package(package, &feature_flags) {
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

            //builds into the staging dir but doesn't copy files into /usr - user will probably never use

            let name = &args[2];
            let packages = load_repo_or_exit();
            let feature_flags = active_feature_flags(&[]);

            match resolve_package_order(&packages, name, &feature_flags) {
                Ok(order) => {
                    for package_name in order {
                        let Some(package) = find_package(&packages, &package_name) else {
                            eprintln!("Error: Package vanished from repo: {}", package_name);
                            return;
                        };

                        if let Err(message) = build_package(package, &feature_flags) {
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

            //downloads, verifies, extracts, builds, then installs the files into the right place

            let name = &args[2];
            let feature_flags = active_feature_flags(&args[3..]);
            let mut packages = load_repo_or_exit();

            let order = match resolve_package_order(&packages, name, &feature_flags) {
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

            let order = match resolve_package_order(&packages, name, &feature_flags) {
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

                if let Err(message) = install_package(package, &feature_flags) {
                    eprintln!("Error: {}", message);
                    return;
                }
            }
        }
        "list" => match load_installed_packages() {

            //list all installed files

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

            //checks for outdated packages against the repo recipe

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

            //checks whether a recipe has a newer upstream version

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

            //rewrites a recipe to the latest detected version

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
            //prints which recipe repo mahou is reading, mainly for debug
            println!("{}", recipe_repo_path());
        }
        "sync" => {

            //checks ALL recipe repos to sync them to latest upstream version

            if let Err(message) = sync_recipe_repo() {
                eprintln!("Error: {}", message);
                return;
            }

            if let Err(message) = sync_upstream_recipes() {
                eprintln!("Error: {}", message);
            }
        }
        "upgrade" => {
            //updates all installed packages

            if let Err(message) = upgrade_installed_packages() {
                eprintln!("Error: {}", message);
            }
        }
        "init-config" => {
            //initialises a config file

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
    //self explanatory right

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

fn print_deps(packages: &[Package], package: &Package, flags: &[String], depth: usize) {
    let indent = "  ".repeat(depth);

    if depth == 0 {
        println!("{}", package.name);
    } else {
        println!("{}└── {}", indent, package.name);
    }

    let mut deps = package.deps.clone();
    deps.extend(enabled_feature_deps(package, flags));

    for dep_name in &deps {
        match find_package(packages, dep_name) {
            Some(dep) => print_deps(packages, dep, flags, depth + 1),
            None => println!("{} └── {} (missing)", indent, dep_name),
        }
    }
}

fn print_features(package: &Package, flags: &[String]) {
    if package.features.is_empty() {
        println!("Features: None");
        return;
    }

    println!("Features:");

    let mut feature_names: Vec<&String> = package.features.keys().collect();
    feature_names.sort();

    for feature_name in feature_names {
        let Some(feature) = package.features.get(feature_name) else {
            continue;
        };

        let status = if feature_enabled(package, feature_name, flags) {
            "enabled"
        } else {
            "disabled"
        };

        if feature.deps.is_empty() {
            println!(" - {}: {}", feature_name, status);
        } else {
            println!(
                " - {}: {} deps: {}",
                feature_name,
                status,
                feature.deps.join(", ")
            );
        }
    }
}

fn resolve_package_order(
    packages: &[Package],
    name: &str,
    flags: &[String],
) -> Result<Vec<String>, String> {
    let mut visited = HashSet::new();
    let mut order = Vec::new();

    resolve_package(packages, name, flags, &mut visited, &mut order)?;

    Ok(order)
}

fn resolve_package(
    packages: &[Package],
    name: &str,
    flags: &[String],
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

    let mut deps = package.deps.clone();
    deps.extend(enabled_feature_deps(package, flags));

    for dep_name in &deps {
        resolve_package(packages, dep_name, flags, visited, order)?;
    }

    order.push(package.name.clone());

    Ok(())
}
