use std::sync::Arc;

use bevy_ecs::component::Component;
use gpui::SceneSnapshot;

/// An immutable retained GPUI scene associated with one Bevy camera.
///
/// With the default `render` feature this component is cheaply cloned into
/// Bevy's pipelined render world. It remains available without rendering for
/// lifecycle and state tests.
#[derive(Clone, Component)]
#[cfg_attr(
    feature = "render",
    derive(bevy_render::extract_component::ExtractComponent)
)]
pub struct GpuiScene {
    /// Immutable renderer-ready snapshot shared with Bevy's render world.
    pub snapshot: Arc<SceneSnapshot>,
    /// Monotonically increasing generation for this retained context.
    pub generation: u64,
}

impl GpuiScene {
    pub(crate) fn new(snapshot: SceneSnapshot, generation: u64) -> Self {
        Self {
            snapshot: Arc::new(snapshot),
            generation,
        }
    }
}
