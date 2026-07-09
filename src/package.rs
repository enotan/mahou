use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct Package {
    pub name: String,

    pub version: String,

    pub description: String,

    pub source: String,

    pub sha256: String,

    pub source_dir: String,

    pub deps: Vec<String>,

    pub build_steps: Vec<String>,

    #[serde(default = "default_build_profile")]
    pub build_profile: String,

    #[serde(default)]
    pub features: HashMap<String, PackageFeature>,

    pub update: Option<UpdateInfo>,
}

#[derive(Debug, Deserialize)]
pub struct PackageFeature {
    #[serde(default = "default_feature_enabled")]
    pub enabled: bool,

    #[serde(default)]
    pub deps: Vec<String>,

    #[serde(default)]
    pub build_flags: Vec<String>,

    #[serde(default)]
    pub disabled_build_flags: Vec<String>,
}

fn default_feature_enabled() -> bool {
    true
}

fn default_build_profile() -> String {
    "native".to_string()
}

#[derive(Debug, Deserialize)]
pub struct UpdateInfo {
    pub check_url: String,
    pub version_pattern: String,
    pub source_template: String,
    pub source_dir_template: String,
}

#[derive(Debug, Deserialize)]
pub struct InstallRecord {
    pub name: String,
    pub version: String,
    pub files: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct MahouConfig {
    pub github_token: Option<String>,
    pub recipe_repo_url: Option<String>,
    pub recipe_repo_path: Option<String>,
    pub cache_dir: Option<String>,
    pub install_db_dir: Option<String>,

    #[serde(default)]
    pub feature_flags: Vec<String>,
}

pub struct RecipeUpdate {
    pub version: String,
    pub source: String,
    pub sha256: String,
    pub source_dir: String,
}
