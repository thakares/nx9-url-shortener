use super::line::generate_line_chart;

/// Generates a timeseries traffic trend SVG chart.
///
/// This provides a specialized wrapper around line charts, specifically
/// configured for timeseries data streams (daily/monthly traffic logs).
pub fn generate_timeseries_chart(data: &[(String, i64)]) -> String {
    generate_line_chart(data)
}
