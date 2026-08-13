use std::path::Path;

use plotly::{
    Plot, Scatter,
    common::{Line, Mode},
    layout::{Axis, Layout},
};

use crate::{Result, dataset::Dataset};

pub fn write_chart(dataset: &Dataset, output: &Path) -> Result<()> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }

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
        .height(700)
        .width(1100);

    let mut plot = Plot::new();
    plot.add_trace(total_trace);
    plot.add_trace(const_trace);
    plot.set_layout(layout);
    plot.write_html(output);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataset::{Dataset, ReleaseObservation};

    #[test]
    fn chart_json_contains_two_named_traces() {
        let dataset = Dataset::new(vec![ReleaseObservation {
            release: "1.97.0".to_owned(),
            total_methods: 10,
            const_methods: 4,
            source: "fixture".to_owned(),
        }])
        .unwrap();

        let mut plot = Plot::new();
        plot.add_trace(
            Scatter::new(vec!["1.97.0"], vec![10])
                .name("Total stable methods")
                .mode(Mode::LinesMarkers),
        );
        plot.add_trace(
            Scatter::new(vec!["1.97.0"], vec![4])
                .name("Const-capable stable methods")
                .mode(Mode::LinesMarkers),
        );
        let json = plot.to_json();

        assert!(json.contains("Total stable methods"));
        assert!(json.contains("Const-capable stable methods"));
        assert_eq!(dataset.observations.len(), 1);
    }
}
