// Donut / Pie SVG Chart Renderer (Placeholder / Stub)

pub fn generate_pie_chart(_data: &[(String, i64)]) -> String {
    r##"<svg viewBox="0 0 400 400" class="w-full h-auto bg-slate-900/50 rounded-xl border border-slate-800/80 p-4" xmlns="http://www.w3.org/2000/svg">
        <circle cx="200" cy="200" r="100" fill="none" stroke="#6366f1" stroke-width="40"/>
        <text x="200" y="200" fill="#94a3b8" text-anchor="middle" font-family="system-ui, sans-serif" alignment-baseline="middle">Donut Chart Placeholder</text>
    </svg>"##.to_string()
}
