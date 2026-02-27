#[cfg(feature = "engine")]
pub mod nodes {

    use crate::Point;
    use daedalus::declare_plugin;
    use daedalus::macros::node;
    use daedalus::runtime::NodeError;
    use image::{DynamicImage, GrayImage};

    // Daedalus's `#[node(...)]` attribute requires literal values; that makes it awkward
    // to fully macro-generate a family of nodes. Instead, keep a single generic
    // implementation and expose thin typed wrappers for the UI.
    fn gate_impl<T>(value: T, enabled: bool) -> Option<T> {
        enabled.then_some(value)
    }

    fn select_impl<T>(primary: T, fallback: T, enabled: bool) -> T {
        if enabled { primary } else { fallback }
    }

    #[node(id = "gate_gray", inputs("value", "enabled"), outputs("value"))]
    fn gate_gray(value: GrayImage, enabled: bool) -> Result<Option<GrayImage>, NodeError> {
        Ok(gate_impl(value, enabled))
    }

    #[node(id = "gate_frame", inputs("value", "enabled"), outputs("value"))]
    fn gate_frame(value: DynamicImage, enabled: bool) -> Result<Option<DynamicImage>, NodeError> {
        Ok(gate_impl(value, enabled))
    }

    #[node(id = "gate_points", inputs("value", "enabled"), outputs("value"))]
    fn gate_points(value: Vec<Point>, enabled: bool) -> Result<Option<Vec<Point>>, NodeError> {
        Ok(gate_impl(value, enabled))
    }

    #[node(id = "select_gray", inputs("primary", "fallback", "enabled"), outputs("value"))]
    fn select_gray(primary: GrayImage, fallback: GrayImage, enabled: bool) -> Result<GrayImage, NodeError> {
        Ok(select_impl(primary, fallback, enabled))
    }

    #[node(id = "select_frame", inputs("primary", "fallback", "enabled"), outputs("value"))]
    fn select_frame(primary: DynamicImage, fallback: DynamicImage, enabled: bool) -> Result<DynamicImage, NodeError> {
        Ok(select_impl(primary, fallback, enabled))
    }

    #[node(id = "select_points", inputs("primary", "fallback", "enabled"), outputs("value"))]
    fn select_points(primary: Vec<Point>, fallback: Vec<Point>, enabled: bool) -> Result<Vec<Point>, NodeError> {
        Ok(select_impl(primary, fallback, enabled))
    }

    declare_plugin!(CvLogicPlugin, "logic", [gate_gray, gate_frame, gate_points, select_gray, select_frame, select_points]);
}
