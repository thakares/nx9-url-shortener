use super::svg::{wrap_in_svg, DEFAULT_GRID_COLOR, DEFAULT_TEXT_COLOR};

pub fn generate_line_chart(data: &[(String, i64)]) -> String {
    if data.is_empty() {
        return r##"<svg viewBox="0 0 800 300" class="w-full h-auto bg-slate-900/50 rounded-xl border border-slate-800/80 p-4" xmlns="http://www.w3.org/2000/svg">
            <text x="400" y="150" fill="#94a3b8" text-anchor="middle" font-family="system-ui, sans-serif">No traffic data available</text>
        </svg>"##.to_string();
    }

    let width = 800.0;
    let height = 300.0;
    let pad_left = 60.0;
    let pad_right = 30.0;
    let pad_top = 30.0;
    let pad_bottom = 40.0;

    let chart_w = width - pad_left - pad_right;
    let chart_h = height - pad_top - pad_bottom;

    // Find max value for scaling
    let max_val = data.iter().map(|(_, v)| *v).max().unwrap_or(0);
    let max_y = if max_val == 0 { 10.0 } else { max_val as f64 };

    // Y-axis grid ticks
    let mut inner_content = String::new();
    let ticks = 4;
    for i in 0..=ticks {
        let pct = i as f64 / ticks as f64;
        let y = pad_top + chart_h - (pct * chart_h);
        let val = (pct * max_y).round() as i64;
        inner_content.push_str(&format!(
            r##"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}" stroke-dasharray="4,4" stroke-width="1"/>
               <text x="{}" y="{}" fill="{}" font-size="11" font-family="system-ui, sans-serif" text-anchor="end" alignment-baseline="middle">{}</text>"##,
            pad_left, y, width - pad_right, y, DEFAULT_GRID_COLOR, pad_left - 10.0, y, DEFAULT_TEXT_COLOR, val
        ));
    }

    // Coordinates calculations
    let count = data.len();
    let step_x = if count > 1 { chart_w / (count - 1) as f64 } else { chart_w };

    let mut points = Vec::new();
    for (i, &(_, val)) in data.iter().enumerate() {
        let x = pad_left + (i as f64 * step_x);
        let y = pad_top + chart_h - ((val as f64 / max_y) * chart_h);
        points.push((x, y));
    }

    // Path strings
    let mut line_path = String::new();
    let mut area_path = String::new();

    if !points.is_empty() {
        line_path.push_str(&format!("M {:.1} {:.1}", points[0].0, points[0].1));
        area_path.push_str(&format!("M {:.1} {:.1}", points[0].0, pad_top + chart_h));
        area_path.push_str(&format!("L {:.1} {:.1}", points[0].0, points[0].1));

        for &(x, y) in points.iter().skip(1) {
            line_path.push_str(&format!(" L {:.1} {:.1}", x, y));
            area_path.push_str(&format!(" L {:.1} {:.1}", x, y));
        }

        let last_x = points[points.len() - 1].0;
        area_path.push_str(&format!(" L {:.1} {:.1} Z", last_x, pad_top + chart_h));
    }

    // X-axis labels
    let mut x_labels = String::new();
    let label_step = (count / 7).max(1);
    for (i, (label, _)) in data.iter().enumerate() {
        if i % label_step == 0 || i == count - 1 {
            let x = points[i].0;
            let short_label = if label.len() == 10 { &label[5..] } else { label };
            x_labels.push_str(&format!(
                r##"<text x="{}" y="{}" fill="{}" font-size="11" font-family="system-ui, sans-serif" text-anchor="middle">{}</text>"##,
                x, height - 15.0, DEFAULT_TEXT_COLOR, short_label
            ));
            
            x_labels.push_str(&format!(
                r##"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}" stroke-width="1"/>"##,
                x, pad_top + chart_h, x, pad_top + chart_h + 5.0, DEFAULT_GRID_COLOR
            ));
        }
    }

    // Draw little circles on points
    let mut dots = String::new();
    if count < 40 {
        for &(x, y) in &points {
            dots.push_str(&format!(
                r##"<circle cx="{:.1}" cy="{:.1}" r="4" fill="#6366f1" stroke="#1e293b" stroke-width="2"/>"##,
                x, y
            ));
        }
    }

    inner_content.push_str(&format!(
        r##"<path d="{}" fill="url(#areaGrad)"/>
        <path d="{}" fill="none" stroke="#6366f1" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/>
        {}
        {}"##,
        area_path, line_path, dots, x_labels
    ));

    wrap_in_svg(width, height, &inner_content)
}
