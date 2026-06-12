use super::svg::{wrap_in_svg, DEFAULT_TEXT_COLOR};

pub fn generate_bar_chart(data: &[(String, i64)]) -> String {
    if data.is_empty() {
        return r##"<svg viewBox="0 0 600 200" class="w-full h-auto bg-slate-900/50 rounded-xl border border-slate-800/80 p-4" xmlns="http://www.w3.org/2000/svg">
            <text x="300" y="100" fill="#94a3b8" text-anchor="middle" font-family="system-ui, sans-serif">No data available</text>
        </svg>"##.to_string();
    }

    let bar_height = 24.0;
    let gap = 14.0;
    let pad_top = 10.0;
    let pad_bottom = 10.0;
    let pad_left = 130.0;
    let pad_right = 70.0;
    let width = 600.0;

    let height = pad_top + pad_bottom + (data.len() as f64 * (bar_height + gap)) - gap;
    let chart_w = width - pad_left - pad_right;

    let max_val = data.iter().map(|(_, v)| *v).max().unwrap_or(0);
    let max_x = if max_val == 0 { 1.0 } else { max_val as f64 };

    let mut bars = String::new();
    let total_count: i64 = data.iter().map(|(_, v)| *v).sum();

    for (i, (label, val)) in data.iter().enumerate() {
        let y = pad_top + (i as f64 * (bar_height + gap));
        let bar_w = (*val as f64 / max_x) * chart_w;

        let pct = if total_count > 0 {
            (*val as f64 / total_count as f64) * 100.0
        } else {
            0.0
        };

        // Truncate long labels
        let display_label = if label.len() > 18 {
            format!("{}...", &label[0..15])
        } else {
            label.to_string()
        };

        bars.push_str(&format!(
            r##"<!-- Row {i} -->
            <text x="{pad_left_label}" y="{text_y}" fill="#e2e8f0" font-size="12" font-family="system-ui, sans-serif" font-weight="500" text-anchor="end" alignment-baseline="middle">{label}</text>
            <rect x="{pad_left}" y="{y}" width="{bar_w:.1}" height="{bar_height}" rx="4" fill="url(#barGrad)"/>
            <text x="{val_x:.1}" y="{text_y}" fill="{text_color}" font-size="11" font-family="system-ui, sans-serif" alignment-baseline="middle">{val} ({pct:.1}%)</text>
            "##,
            i = i,
            pad_left_label = pad_left - 10.0,
            text_y = y + (bar_height / 2.0) + 1.0,
            label = display_label,
            pad_left = pad_left,
            y = y,
            bar_w = bar_w.max(2.0),
            bar_height = bar_height,
            val_x = pad_left + bar_w.max(2.0) + 8.0,
            val = val,
            pct = pct,
            text_color = DEFAULT_TEXT_COLOR
        ));
    }

    wrap_in_svg(width, height, &bars)
}
