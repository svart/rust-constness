mod artifact;
pub mod cache;
pub mod chart;
pub mod collector;
pub mod dataset;
pub mod html;
pub mod rustdoc;
pub mod versions;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
