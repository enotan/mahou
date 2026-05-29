use crate::build::{install_package, load_installed_packages, sha256_bytes};

use crate::config::{
    active_feature_flags, add_auth_header, http_client, recipe_checkout_path, recipe_repo_path,
    recipe_repo_url,
};

use crate::package::{Package, RecipeUpdate};

use crate::repo::{find_package, load_repo_or_exit};

use regex::Regex;
use std::cmp::Ordering;
use std::fs;
use std::process::Command;

pub fn check_for_update(package: &Package) -> Result<Option<String>, String> {
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

pub fn plan_recipe_update(package: &Package, latest: &str) -> Result<RecipeUpdate, String> {
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

pub fn write_recipe_update(package: &Package, plan: &RecipeUpdate) -> Result<(), String> {
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

pub fn recipe_path(name: &str) -> String {
    format!("{}/{}.toml", recipe_repo_path(), name)
}

pub fn update_recipe_if_needed(package: &Package) -> Result<bool, String> {
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

pub fn refresh_recipes_for_order(packages: &[Package], order: &[String]) -> Result<bool, String> {
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

pub fn sync_recipe_repo() -> Result<(), String> {
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

pub fn sync_upstream_recipes() -> Result<(), String> {
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

pub fn refresh_installed_recipes() -> Result<(), String> {
    let packages = load_repo_or_exit();
    let installed = load_installed_packages()?;

    if installed.is_empty() {
        println!("No packages installed");
        return Ok(());
    }

    let mut checked = 0;
    let mut updated = 0;
    let mut failed = 0;

    for record in installed {
        let Some(package) = find_package(&packages, &record.name) else {
            println!("Installed package has no recipe: {}", record.name);
            continue;
        };

        checked += 1;

        match update_recipe_if_needed(package) {
            Ok(true) => updated += 1,
            Ok(false) => {}
            Err(message) => {
                failed += 1;
                eprintln!("Warning: failed to refresh {}: {}", package.name, message);
            }
        }
    }

    println!(
        "Refresh complete: checked {}, updated {}, failed {}",
        checked, updated, failed
    );

    Ok(())
}

pub fn upgrade_installed_packages() -> Result<(), String> {
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

        let feature_flags = active_feature_flags(&[]);
        install_package(package, &feature_flags)?;
        upgraded = true;
    }

    if !upgraded {
        println!("All installed packages are up to date");
    }

    Ok(())
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
