pub mod svg;
pub mod line;
pub mod bar;
pub mod pie;
pub mod timeseries;

pub use line::generate_line_chart;
pub use bar::generate_bar_chart;
pub use pie::generate_pie_chart;
pub use timeseries::generate_timeseries_chart;
