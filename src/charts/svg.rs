pub const DEFAULT_GRID_COLOR: &str = "#334155";
pub const DEFAULT_TEXT_COLOR: &str = "#94a3b8";

pub fn wrap_in_svg(width: f64, height: f64, content: &str) -> String {
    // Note: raw string literals with # inside double-quotes can break standard parsing,
    // so we use r##"..."## formatting to prevent compiler errors.
    format!(
        r##"<svg viewBox="0 0 {width} {height}" class="w-full h-auto" xmlns="http://www.w3.org/2000/svg">
            <defs>
                <linearGradient id="areaGrad" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stop-color="#6366f1" stop-opacity="0.35"/>
                    <stop offset="100%" stop-color="#6366f1" stop-opacity="0.01"/>
                </linearGradient>
                <linearGradient id="barGrad" x1="0" y1="0" x2="1" y2="0">
                    <stop offset="0%" stop-color="#4f46e5"/>
                    <stop offset="100%" stop-color="#818cf8"/>
                </linearGradient>
            </defs>
            {content}
        </svg>"##,
        width = width,
        height = height,
        content = content
    )
}
