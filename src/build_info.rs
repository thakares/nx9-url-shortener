pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_COMMIT: &str = match option_env!("BZOD_GIT_COMMIT") {
    Some(commit) => commit,
    None => "unknown",
};
