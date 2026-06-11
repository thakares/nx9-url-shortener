use std::time::Duration;

// Formats a duration as "Xd Xh Xm Xs"
pub fn format_duration(d: Duration) -> String {
    format!(
        "{}d {}h {}m {}s",
        d.as_secs() / 86400,
        (d.as_secs() % 86400) / 3600,
        (d.as_secs() % 3600) / 60,
        d.as_secs() % 60
    )
}
