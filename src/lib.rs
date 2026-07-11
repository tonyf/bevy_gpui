#![warn(missing_docs)]

//! Retained-mode [`gpui`] overlays for ordinary Bevy applications.
//!
//! [`GpuiPlugin`] is intentionally a normal Bevy plugin. It does not replace
//! Bevy's application runner, create a native window, or own presentation.
//! Bevy remains responsible for its event loop, windows, schedules, renderer,
//! and GPU resources.
//!
//! # Start here
//!
//! Add [`GpuiPlugin`] after Bevy's normal plugins, create a camera, and attach
//! one retained root with [`GpuiContexts::set_root`]. Import GPUI elements from
//! this crate's [`gpui`] re-export so every type comes from the pinned revision.
//!
//! ```rust,no_run
//! use bevy::prelude::*;
//! use bevy_gpui::{
//!     GpuiContexts, GpuiPlugin,
//!     gpui::{Context, IntoElement, Render, Window, div, prelude::*},
//! };
//!
//! fn main() {
//!     App::new()
//!         .add_plugins((DefaultPlugins, GpuiPlugin::default()))
//!         .add_systems(Startup, setup)
//!         .run();
//! }
//!
//! fn setup(mut commands: Commands, mut gpui: GpuiContexts) {
//!     let camera = commands.spawn(Camera2d).id();
//!     gpui.set_root(camera, |_, cx| cx.new(|_| Panel))
//!         .expect("GPUI root should be queued");
//! }
//!
//! struct Panel;
//!
//! impl Render for Panel {
//!     fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
//!         div().child("Hello from GPUI")
//!     }
//! }
//! ```
//!
//! # Input ownership
//!
//! Bevy input messages are broadcast. Order raw gameplay input systems after
//! [`GpuiSystems::Input`]. Systems with message readers must always drain them
//! and inspect [`GpuiInputState`] before acting; the public run conditions suit
//! polling systems without message cursors. With the default `picking` feature,
//! a claimed GPUI window emits a blocker at `f32::MAX` for every picking pointer
//! located in that window.
//!
//! # Guides
//!
//! - [Getting started](https://github.com/tonyf/bevy_gpui/blob/main/docs/getting-started.md)
//! - [Public API reference](https://github.com/tonyf/bevy_gpui/blob/main/docs/reference.md)
//! - [Input and picking](https://github.com/tonyf/bevy_gpui/blob/main/docs/how-to-input-and-picking.md)
//! - [Architecture](https://github.com/tonyf/bevy_gpui/blob/main/docs/architecture.md)
//! - [Compatibility and limitations](https://github.com/tonyf/bevy_gpui/blob/main/docs/compatibility.md)

use std::collections::HashSet;

use bevy_app::{App, Plugin, PostUpdate, PreUpdate};
#[cfg(feature = "picking")]
use bevy_camera::NormalizedRenderTarget;
#[cfg(feature = "picking")]
use bevy_ecs::{
    entity::ContainsEntity,
    prelude::{Entity, MessageWriter, Query, Res},
};
use bevy_ecs::{
    prelude::Resource,
    schedule::{IntoScheduleConfigs, SystemSet},
};
use bevy_input::InputSystems;
#[cfg(feature = "picking")]
use bevy_picking::{
    Pickable, PickingPlugin, PickingSystems,
    backend::{HitData, PointerHits},
    pointer::{Location, PointerId, PointerLocation},
};
#[cfg(feature = "render")]
use bevy_render::{RenderApp, extract_component::ExtractComponentPlugin};

mod bridge;
mod dispatcher;
#[cfg(feature = "render")]
mod image;
mod platform;
#[cfg(feature = "render")]
mod render;
mod runtime;
mod scene;

pub use bridge::BevyAppContextExt;
#[cfg(feature = "render")]
pub use image::{GpuiBevyImage, bevy_image};
pub use runtime::{
    GpuiContext, GpuiContexts, GpuiRuntimeStatus, GpuiViewHandle, PrimaryGpuiContext,
};
pub use scene::GpuiScene;

/// Re-export of the pinned GPUI revision used by this integration.
pub use gpui;

/// Configuration for [`GpuiPlugin`].
#[derive(Clone, Copy, Debug)]
pub struct GpuiPlugin {
    /// Ordering of the GPUI overlay relative to Bevy UI.
    pub render_order: GpuiRenderOrder,
    /// Whether the plugin should attach a context to the primary camera when
    /// no explicit GPUI context is present.
    pub auto_attach_primary: bool,
    /// Accessibility integration mode.
    pub accessibility: GpuiAccessibility,
}

impl Default for GpuiPlugin {
    fn default() -> Self {
        Self {
            render_order: GpuiRenderOrder::AboveBevyUi,
            auto_attach_primary: true,
            accessibility: GpuiAccessibility::Disabled,
        }
    }
}

/// Ordering of GPUI relative to Bevy's built-in UI render pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GpuiRenderOrder {
    /// Render GPUI before Bevy UI.
    BelowBevyUi,
    /// Render GPUI after Bevy UI.
    #[default]
    AboveBevyUi,
}

/// Accessibility behavior for the embedded GPUI tree.
///
/// Bevy owns the native AccessKit adapter. Merging a GPUI subtree is not yet
/// exposed by either framework, so the only truthful mode is currently
/// disabled rather than installing a conflicting second adapter.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GpuiAccessibility {
    /// Do not install a second native accessibility adapter.
    #[default]
    Disabled,
}

#[cfg(feature = "render")]
#[derive(Clone, Copy, Resource)]
struct GpuiPluginSettings {
    auto_attach_primary: bool,
}

#[cfg(feature = "picking")]
#[derive(Resource)]
struct GpuiPickingBlocker(Entity);

impl Plugin for GpuiPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "render")]
        assert!(
            app.get_sub_app(RenderApp).is_some(),
            "GpuiPlugin must be added after Bevy's RenderPlugin (normally after DefaultPlugins)"
        );
        #[cfg(all(feature = "render", not(test)))]
        assert!(
            app.is_plugin_added::<bevy_window::WindowPlugin>(),
            "GpuiPlugin must be added after Bevy's WindowPlugin (normally after DefaultPlugins)"
        );
        #[cfg(all(feature = "render", not(test)))]
        assert!(
            app.is_plugin_added::<bevy_winit::WinitPlugin>(),
            "GpuiPlugin must be added after Bevy's WinitPlugin (normally after DefaultPlugins)"
        );

        #[cfg(feature = "render")]
        let gpu_bridge = render::GpuiGpuBridge::default();
        #[cfg(feature = "render")]
        let image_registry = image::GpuiImageRegistry::default();
        #[cfg(feature = "render")]
        app.add_plugins(ExtractComponentPlugin::<GpuiScene>::default())
            .insert_resource(gpu_bridge.clone())
            .insert_resource(image_registry.clone());

        app.init_resource::<GpuiInputState>()
            .init_resource::<GpuiRuntimeStatus>()
            .insert_non_send(runtime::GpuiRuntimeState::default())
            .configure_sets(
                PreUpdate,
                (
                    GpuiSystems::DriveExecutor,
                    GpuiSystems::Input.after(InputSystems),
                    GpuiSystems::ApplyDeferredBridge,
                )
                    .chain(),
            )
            .configure_sets(
                PostUpdate,
                (
                    GpuiSystems::WindowSync.after(bevy_camera::CameraUpdateSystems),
                    GpuiSystems::BuildScene,
                )
                    .chain(),
            );
        #[cfg(feature = "render")]
        app.add_systems(
            PreUpdate,
            (
                runtime::auto_attach_primary,
                runtime::initialize_runtime,
                runtime::drive_executor,
            )
                .chain()
                .in_set(GpuiSystems::DriveExecutor),
        );
        #[cfg(feature = "render")]
        app.insert_resource(GpuiPluginSettings {
            auto_attach_primary: self.auto_attach_primary,
        });
        #[cfg(not(feature = "render"))]
        app.add_systems(
            PreUpdate,
            runtime::drive_executor.in_set(GpuiSystems::DriveExecutor),
        );

        app.add_systems(
            PreUpdate,
            runtime::dispatch_input.in_set(GpuiSystems::Input),
        )
        .add_systems(PreUpdate, runtime::prepare_window_close)
        .add_systems(
            PreUpdate,
            runtime::apply_deferred_bridge.in_set(GpuiSystems::ApplyDeferredBridge),
        )
        .add_systems(
            PostUpdate,
            runtime::cleanup_orphaned_window_contexts.before(bevy_camera::CameraUpdateSystems),
        )
        .add_systems(
            PostUpdate,
            runtime::sync_windows.in_set(GpuiSystems::WindowSync),
        )
        .add_systems(
            PostUpdate,
            runtime::build_scenes.in_set(GpuiSystems::BuildScene),
        );

        #[cfg(feature = "picking")]
        if app.is_plugin_added::<PickingPlugin>() {
            let blocker = app
                .world_mut()
                .spawn(Pickable {
                    should_block_lower: true,
                    is_hoverable: false,
                })
                .id();
            app.insert_resource(GpuiPickingBlocker(blocker))
                .add_systems(
                    PreUpdate,
                    gpui_picking_backend
                        .in_set(PickingSystems::Backend)
                        .after(GpuiSystems::Input),
                );
        }

        #[cfg(feature = "render")]
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        #[cfg(feature = "render")]
        render_app.insert_resource(gpu_bridge);
        #[cfg(feature = "render")]
        render_app.insert_resource(image_registry);
        #[cfg(feature = "render")]
        render_app.init_resource::<render::GpuiRenderState>();
        #[cfg(feature = "render")]
        render_app.add_systems(bevy_render::RenderStartup, render::initialize_gpui_gpu);
        #[cfg(feature = "render")]
        render_app.add_systems(
            bevy_render::Render,
            render::cleanup_gpui_renderers.in_set(bevy_render::RenderSystems::Cleanup),
        );

        #[cfg(feature = "render")]
        let pass_2d = render::gpui_pass
            .after(bevy_core_pipeline::Core2dSystems::MainPass)
            .before(bevy_core_pipeline::upscaling::upscaling);
        #[cfg(feature = "render")]
        let pass_3d = render::gpui_pass
            .after(bevy_core_pipeline::Core3dSystems::EarlyPostProcess)
            .before(bevy_core_pipeline::upscaling::upscaling);
        #[cfg(feature = "render")]
        match self.render_order {
            GpuiRenderOrder::BelowBevyUi => {
                render_app.add_systems(
                    bevy_core_pipeline::Core2d,
                    pass_2d.before(bevy_ui_render::ui_pass),
                );
                render_app.add_systems(
                    bevy_core_pipeline::Core3d,
                    pass_3d.before(bevy_ui_render::ui_pass),
                );
            }
            GpuiRenderOrder::AboveBevyUi => {
                render_app.add_systems(
                    bevy_core_pipeline::Core2d,
                    pass_2d.after(bevy_ui_render::ui_pass),
                );
                render_app.add_systems(
                    bevy_core_pipeline::Core3d,
                    pass_3d.after(bevy_ui_render::ui_pass),
                );
            }
        }
    }
}

/// Public schedule boundaries used by the integration.
///
/// Applications can order gameplay and synchronization systems relative to
/// these sets without depending on implementation-private system names.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, SystemSet)]
pub enum GpuiSystems {
    /// Poll GPUI foreground tasks and expired timers.
    DriveExecutor,
    /// Translate Bevy window/input messages and dispatch them to GPUI.
    Input,
    /// Apply commands and messages queued by retained GPUI callbacks.
    ApplyDeferredBridge,
    /// Synchronize Bevy window and camera state into GPUI adapter windows.
    WindowSync,
    /// Lay out and paint dirty GPUI views into extractable scene snapshots.
    BuildScene,
}

/// Input ownership reported by the most recent GPUI input pass.
///
/// Bevy publishes input messages to every reader, so the integration exposes
/// claims and run conditions instead of pretending it can erase messages that
/// have already been emitted. Pointer ownership may persist across frames until
/// a later pointer event or window teardown clears it.
#[derive(Clone, Debug, Default, Resource)]
pub struct GpuiInputState {
    /// The latest routed pointer state contains a GPUI hit or capture claim.
    pub wants_pointer_input: bool,
    /// GPUI currently wants keyboard input for its focused element.
    pub wants_keyboard_input: bool,
    /// At least one GPUI dispatch called `prevent_default` in the latest pass.
    pub default_prevented: bool,
    pointer_claims: HashSet<bevy_ecs::entity::Entity>,
}

/// Run condition for pointer-driven gameplay systems.
#[must_use]
pub fn gpui_wants_pointer_input(state: Option<bevy_ecs::prelude::Res<GpuiInputState>>) -> bool {
    state.is_some_and(|state| state.wants_pointer_input)
}

/// Run condition for keyboard-driven gameplay systems.
#[must_use]
pub fn gpui_wants_keyboard_input(state: Option<bevy_ecs::prelude::Res<GpuiInputState>>) -> bool {
    state.is_some_and(|state| state.wants_keyboard_input)
}

#[cfg(feature = "picking")]
fn gpui_picking_backend(
    blocker: Res<GpuiPickingBlocker>,
    input: Res<GpuiInputState>,
    pointers: Query<(&PointerId, &PointerLocation)>,
    mut hits: MessageWriter<PointerHits>,
) {
    for (pointer, location) in &pointers {
        if let Some(location) = location.location()
            && let Some(hit) = gpui_picking_hit(*pointer, location, blocker.0, &input)
        {
            hits.write(hit);
        }
    }
}

#[cfg(feature = "picking")]
fn gpui_picking_hit(
    pointer: PointerId,
    location: &Location,
    blocker: Entity,
    input: &GpuiInputState,
) -> Option<PointerHits> {
    let NormalizedRenderTarget::Window(window) = &location.target else {
        return None;
    };
    input.pointer_claims.contains(&window.entity()).then(|| {
        PointerHits::new(
            pointer,
            vec![(
                blocker,
                HitData::new(blocker, 0.0, Some(location.position.extend(0.0)), None),
            )],
            f32::MAX,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "picking")]
    #[test]
    fn picking_hit_blocks_lower_entities_only_for_claimed_window() {
        use bevy_camera::RenderTarget;
        use bevy_window::{WindowRef, WindowResolution};

        let mut app = App::new();
        let claimed_window = app
            .world_mut()
            .spawn(bevy_window::Window {
                resolution: WindowResolution::new(320, 240),
                ..Default::default()
            })
            .id();
        let other_window = app.world_mut().spawn(bevy_window::Window::default()).id();
        let blocker = app.world_mut().spawn_empty().id();
        let mut input = GpuiInputState::default();
        input.pointer_claims.insert(claimed_window);
        let location = Location {
            target: RenderTarget::Window(WindowRef::Entity(claimed_window))
                .normalize(None)
                .expect("entity window target should normalize"),
            position: bevy_math::Vec2::new(12.0, 34.0),
        };

        let hit = gpui_picking_hit(PointerId::Mouse, &location, blocker, &input)
            .expect("claimed window should emit a blocker hit");
        assert_eq!(hit.pointer, PointerId::Mouse);
        assert_eq!(hit.picks[0].0, blocker);
        assert_eq!(hit.order, f32::MAX);

        let other_location = Location {
            target: RenderTarget::Window(WindowRef::Entity(other_window))
                .normalize(None)
                .expect("entity window target should normalize"),
            position: bevy_math::Vec2::ZERO,
        };
        assert!(gpui_picking_hit(PointerId::Mouse, &other_location, blocker, &input).is_none());
    }

    #[test]
    fn plugin_is_an_ordinary_bevy_plugin() {
        let mut app = App::new();
        #[cfg(feature = "render")]
        app.insert_sub_app(RenderApp, bevy_app::SubApp::new());
        app.add_plugins(GpuiPlugin::default());

        assert!(app.world().contains_resource::<GpuiInputState>());
    }

    #[test]
    fn plugin_installs_public_schedule_sets() {
        let mut app = App::new();
        #[cfg(feature = "render")]
        app.insert_sub_app(RenderApp, bevy_app::SubApp::new());
        app.add_plugins(GpuiPlugin::default());
    }

    #[cfg(feature = "render")]
    #[test]
    #[should_panic(expected = "GpuiPlugin must be added after Bevy's RenderPlugin")]
    fn render_feature_rejects_installation_before_bevy_renderer() {
        let mut app = App::new();
        app.add_plugins(GpuiPlugin::default());
    }
}
