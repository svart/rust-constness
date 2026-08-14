use std::path::Path;

use plotly::{
    Plot, Scatter,
    common::{Anchor, Line, Mode, Orientation},
    layout::{Axis, Layout, Legend},
};

use crate::{Result, dataset::Dataset};

pub fn write_chart(dataset: &Dataset, output: &Path) -> Result<()> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    build_plot(dataset).write_html(output);
    Ok(())
}

fn build_plot(dataset: &Dataset) -> Plot {
    let releases = dataset
        .observations
        .iter()
        .map(|observation| observation.release.clone())
        .collect::<Vec<_>>();
    let total_methods = dataset
        .observations
        .iter()
        .map(|observation| observation.total_methods)
        .collect::<Vec<_>>();
    let const_methods = dataset
        .observations
        .iter()
        .map(|observation| observation.const_methods)
        .collect::<Vec<_>>();

    let total_trace = Scatter::new(releases.clone(), total_methods)
        .mode(Mode::LinesMarkers)
        .name("Total stable methods")
        .line(Line::new().color("#2563eb"));
    let const_trace = Scatter::new(releases, const_methods)
        .mode(Mode::LinesMarkers)
        .name("Const-capable stable methods")
        .line(Line::new().color("#dc2626"));

    let layout = Layout::new()
        .title("Rust standard-library method constness")
        .x_axis(Axis::new().title("Rust release"))
        .y_axis(Axis::new().title("Number of methods"))
        .legend(
            Legend::new()
                .orientation(Orientation::Horizontal)
                .x(0.5)
                .x_anchor(Anchor::Center)
                .y(1.02)
                .y_anchor(Anchor::Bottom),
        )
        .auto_size(true)
        .height(700);

    let mut plot = Plot::new();
    plot.add_trace(total_trace);
    plot.add_trace(const_trace);
    plot.set_layout(layout);
    plot
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataset::{Dataset, ReleaseObservation};

    #[test]
    fn chart_uses_a_top_legend_and_the_available_page_width() {
        let dataset = Dataset::new(vec![ReleaseObservation {
            release: "1.97.0".to_owned(),
            total_methods: 10,
            const_methods: 4,
            source: "fixture".to_owned(),
        }])
        .unwrap();

        let chart: serde_json::Value =
            serde_json::from_str(&build_plot(&dataset).to_json()).unwrap();
        let layout = &chart["layout"];

        assert_eq!(layout["autosize"], true);
        assert!(layout.get("width").is_none());
        assert_eq!(layout["legend"]["orientation"], "h");
        assert_eq!(layout["legend"]["x"], 0.5);
        assert_eq!(layout["legend"]["xanchor"], "center");
        assert_eq!(layout["legend"]["y"], 1.02);
        assert_eq!(layout["legend"]["yanchor"], "bottom");
        assert_eq!(chart["data"][0]["name"], "Total stable methods");
        assert_eq!(chart["data"][1]["name"], "Const-capable stable methods");
    }
}
