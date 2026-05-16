use std::env;
use std::fs;

#[derive(Debug)]
struct Package {
    name: String,
    version: String,
    description: String,
    source: String,
    deps: Vec<String>,
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
            let packages = load_packages("repo");

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
            let packages = load_packages("repo");

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
                }
                None => {
                    eprintln!("Package not found: {}", name);
                }
            }
        }
        "build" => {
            println!("Building package...");

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
}

fn parse_package(contents: &str) -> Package {
    let mut name = String::new();
    let mut version = String::new();
    let mut description = String::new();
    let mut source = String::new();
    let mut deps = Vec::new();

    for line in contents.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        match key {
            "name" => name = value.to_string(),
            "version" => version = value.to_string(),
            "description" => description = value.to_string(),
            "source" => source = value.to_string(),
            "deps" => {
                if !value.is_empty() {
                    deps = value.
                        split(',')
                        .map(|dep| dep.trim().to_string())
                        .collect();
                }
            }
            _ => {}
        }
    }

    Package {
        name,
        version,
        description,
        source,
        deps,
    }
}

fn load_packages(repo_path: &str) -> Vec<Package> {
    let mut packages = Vec::new();

    let entries = fs::read_dir(repo_path).expect("failed to read repo directory");

    for entry in entries {
        let entry = entry.expect("failed to read directory entry");
        let path = entry.path();

        if path.extension().and_then(|ext| ext.to_str()) != Some("pkg") {
            continue;
        }

        let contents = fs::read_to_string(&path).expect("failed to read package file");
        let package = parse_package(&contents);

        packages.push(package);
    }

    packages
}

fn find_packages<'a>(packages: &'a [Package], name: &str) -> Option<&'a Package> {
    packages.iter().find(|package| package.name == name)
}