use crate::package::Package;

pub fn feature_enabled(package: &Package, feature_name: &str, flags: &[String]) -> bool {
    let Some(feature) = package.features.get(feature_name) else {
        return false;
    };

    let global_enable = feature_name.to_string();
    let global_disable = format!("-{}", feature_name);
    let package_enable = format!("{}.{}", package.name, feature_name);
    let package_disable = format!("-{}.{}", package.name, feature_name);

    if flags.contains(&package_disable) {
        return false;
    }

    if flags.contains(&package_enable) {
        return true;
    }

    if flags.contains(&global_disable) {
        return false;
    }

    if flags.contains(&global_enable) {
        return true;
    }

    feature.enabled
}

pub fn enabled_feature_deps(package: &Package, flags: &[String]) -> Vec<String> {
    let mut deps = Vec::new();

    for (feature_name, feature) in &package.features {
        if feature_enabled(package, feature_name, flags) {
            deps.extend(feature.deps.clone());
        }
    }

    deps
}

pub fn feature_build_flags(package: &Package, flags: &[String]) -> Vec<String> {
    let mut build_flags = Vec::new();

    for (feature_name, feature) in &package.features {
        if feature_enabled(package, feature_name, flags) {
            build_flags.extend(feature.build_flags.clone());
        } else {
            build_flags.extend(feature.disabled_build_flags.clone());
        }
    }

    build_flags
}

pub fn expand_build_step(package: &Package, step: &str, flags: &[String]) -> String {
    let feature_flags = feature_build_flags(package, flags).join(" ");

    step.replace("{feature_flags}", &feature_flags)
        .replace("{name}", &package.name)
        .replace("{version}", &package.version)
        .replace("{source_dir}", &package.source_dir)
}
