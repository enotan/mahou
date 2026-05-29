use crate::package::{MahouConfig, Package};
use std::env;
use std::fs;
use std::io::{self, Write};

pub fn load_config() -> MahouConfig {
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

pub fn active_feature_flags(extra_flags: &[String]) -> Vec<String> {
    let mut flags = load_config().feature_flags;
    flags.extend(extra_flags.iter().cloned());
    flags
}

pub fn cache_dir() -> String {
    load_config()
        .cache_dir
        .unwrap_or_else(|| default_cache_dir().to_string())
}

pub fn distfiles_dir() -> String {
    format!("{}/distfiles", cache_dir())
}

pub fn build_dir() -> String {
    format!("{}/build", cache_dir())
}

pub fn stage_root() -> String {
    format!("{}/stage", cache_dir())
}

pub fn stage_dir(package: &Package) -> String {
    format!("{}/{}", stage_root(), package.name)
}

pub fn recipe_repo_path() -> String {
    if fs::metadata("repo").is_ok() {
        "repo".to_string()
    } else {
        load_config()
            .recipe_repo_path
            .unwrap_or_else(|| default_recipe_repo_path().to_string())
    }
}

pub fn recipe_repo_url() -> String {
    load_config()
        .recipe_repo_url
        .unwrap_or_else(|| default_recipe_repo_url().to_string())
}

pub fn recipe_checkout_path() -> String {
    let recipe_path = load_config()
        .recipe_repo_path
        .unwrap_or_else(|| default_recipe_repo_path().to_string());

    let path = std::path::Path::new(&recipe_path);

    if path.file_name().and_then(|name| name.to_str()) == Some("repo") {
        return path.parent().unwrap_or(path).to_string_lossy().to_string();
    }

    recipe_path
}

pub fn github_token() -> Option<String> {
    env::var("GITHUB_TOKEN")
        .ok()
        .or_else(|| load_config().github_token)
}

pub fn http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent("mahou/0.2")
        .build()
        .map_err(|error| format!("Failed to create HTTP client: {}", error))
}

pub fn add_auth_header(
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

pub fn init_config() -> Result<(), String> {
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

# Optional package features are enabled by default when recipes declare them.
# Prefix a feature with "-" to disable it globally.
# Use "package.feature" or "-package.feature" for package-specific overrides.
#
# Examples:
# feature_flags = [
#   "-bluetooth",
#   "-waybar.pipewire",
#   "waybar.pulseaudio",
# ]
feature_flags = []
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

pub fn default_recipe_repo_url() -> &'static str {
    "https://github.com/enotan/mahou-recipes.git"
}

pub fn default_recipe_repo_path() -> &'static str {
    "/var/lib/mahou/repos/main/repo"
}

pub fn default_cache_dir() -> &'static str {
    "/var/cache/mahou"
}

pub fn config_path() -> String {
    env::var("MAHOU_CONFIG").unwrap_or_else(|_| "/etc/mahou/config.toml".to_string())
}

pub fn install_db_dir() -> String {
    load_config()
        .install_db_dir
        .unwrap_or_else(|| default_install_db_dir().to_string())
}

pub fn default_install_db_dir() -> &'static str {
    "/var/lib/mahou/installed"
}
