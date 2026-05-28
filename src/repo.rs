use crate::config::recipe_repo_path;
use crate::package::Package;
use std::fs;

pub fn load_packages(repo_path: &str) -> Result<Vec<Package>, String> {
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

pub fn find_package<'a>(packages: &'a [Package], name: &str) -> Option<&'a Package> {
    packages.iter().find(|package| package.name == name)
}

pub fn load_repo_or_exit() -> Vec<Package> {
    let repo_path = recipe_repo_path();

    match load_packages(&repo_path) {
        Ok(packages) => packages,
        Err(message) => {
            eprintln!("{}", message);
            std::process::exit(1);
        }
    }
}
