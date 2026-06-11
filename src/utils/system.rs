use std::path::Path;

// Read memory usage on Linux
pub fn get_memory_usage() -> String {
    if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
        let fields: Vec<&str> = statm.split_whitespace().collect();
        if !fields.is_empty() {
            if let Ok(pages) = fields[0].parse::<u64>() {
                // Page size is usually 4096 bytes
                let bytes = pages * 4096;
                return format!("{:.2} MB", bytes as f64 / 1_048_576.0);
            }
        }
    }
    "N/A".to_string()
}

// Generate diagnostic size report for the SQLite files
pub fn get_db_file_info(data_dir: &Path) -> String {
    let mut stats = String::new();
    let files = vec![
        ("admin.db", "Admin DB"),
        ("content.db", "Content DB"),
        ("analytics.db", "Analytics DB"),
        ("system.db", "System DB"),
    ];

    for (f, name) in files {
        let path = data_dir.join(f);
        if path.exists() {
            if let Ok(metadata) = std::fs::metadata(&path) {
                let size = metadata.len();
                stats.push_str(&format!("{}: {} bytes ({:.2} MB)\n", name, size, size as f64 / 1_048_576.0));
            }
        } else {
            stats.push_str(&format!("{}: File not created yet\n", name));
        }
    }
    stats
}
