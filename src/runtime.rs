#[cfg(feature = "render")]
use std::sync::Arc;
use std::{
    collections::{HashMap, HashSet},
    marker::PhantomData,
    rc::Rc,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow};
use bevy_app::AppExit;
use bevy_camera::{Camera, NormalizedRenderTarget, RenderTarget};
#[cfg(feature = "render")]
use bevy_ecs::prelude::Res;
use bevy_ecs::{
    component::Component,
    entity::{ContainsEntity, Entity as BevyEntity},
    prelude::{
        Commands, MessageReader, MessageWriter, NonSendMut, Query, ResMut, Resource, With, Without,
    },
    system::SystemParam,
};
use bevy_input::{
    ButtonState,
    gestures::PinchGesture,
    keyboard::{Key as BevyKey, KeyboardInput},
    mouse::{MouseButton as BevyMouseButton, MouseButtonInput, MouseScrollUnit, MouseWheel},
    touch::TouchPhase as BevyTouchPhase,
};
use bevy_math::{Rect, Vec2};
use bevy_window::{
    AppLifecycle, CursorEntered, CursorIcon, CursorLeft, CursorMoved, CursorOptions,
    FileDragAndDrop, Ime, Monitor as BevyMonitor, MonitorSelection, OnMonitor, PrimaryMonitor,
    PrimaryWindow, RawHandleWrapper, SystemCursorIcon, Window as BevyWindow, WindowCloseRequested,
    WindowFocused, WindowMode, WindowMoved, WindowTheme, WindowThemeChanged,
};
#[cfg(feature = "render")]
use bevy_winit::{EventLoopProxyWrapper, WinitUserEvent};
#[cfg(feature = "render")]
use gpui::Application;
use gpui::{
    AnyWindowHandle, App, Capslock, Context, EmbeddedApplication, Entity, ExternalPaths,
    FileDropEvent, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, ModifiersChangedEvent,
    MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, NavigationDirection,
    PinchEvent, PlatformInput, Render, ScrollDelta, ScrollWheelEvent, TouchPhase, Window,
    WindowOptions, point, px,
};
#[cfg(feature = "render")]
use gpui::{AppContext as _, div};

#[cfg(feature = "render")]
use crate::GpuiPluginSettings;
#[cfg(feature = "render")]
use crate::bridge::BevyBridgeGlobal;
use crate::bridge::BevyMutationQueue;
#[cfg(feature = "render")]
use crate::image::GpuiImageRegistry;
#[cfg(feature = "render")]
use crate::render::GpuiGpuBridge;
use crate::{GpuiInputState, GpuiScene, platform::BevyPlatform};

type RootFactory = Rc<dyn Fn(&mut GpuiRuntime) -> Result<()>>;

/// Observable lifecycle state for the embedded GPUI runtime.
#[derive(Clone, Copy, Debug, Default, Resource)]
pub struct GpuiRuntimeStatus {
    /// The shared GPU atlas and embedded application are initialized.
    pub ready: bool,
    /// Number of retained roots materialized in GPUI after the latest window sync.
    pub roots: usize,
    /// Number of GPUI scene snapshots delivered to Bevy cameras.
    pub scenes_built: u64,
    /// Generation of Bevy's render device currently used by GPUI.
    pub gpu_generation: u64,
    /// Number of times the embedded runtime recovered onto a replacement GPU.
    pub recoveries: u64,
}

#[cfg_attr(not(feature = "render"), allow(dead_code))]
pub(crate) struct GpuiRuntimeState {
    next_id: u64,
    root_factories: HashMap<u64, RootFactory>,
    gpu_generation: Option<u64>,
    runtime: Option<GpuiRuntime>,
    bridge: BevyMutationQueue,
}

impl Default for GpuiRuntimeState {
    fn default() -> Self {
        Self {
            next_id: 1,
            root_factories: HashMap::new(),
            gpu_generation: None,
            runtime: None,
            bridge: BevyMutationQueue::default(),
        }
    }
}

impl Drop for GpuiRuntimeState {
    fn drop(&mut self) {
        shutdown_runtime(self.runtime.take());
    }
}

fn shutdown_runtime(runtime: Option<GpuiRuntime>) {
    if let Some(runtime) = runtime
        && let Err(error) = runtime.application.shutdown()
    {
        bevy_log::warn!(?error, "failed to shut down embedded GPUI runtime cleanly");
    }
}

struct GpuiRuntime {
    application: EmbeddedApplication,
    platform: Rc<BevyPlatform>,
    windows: HashMap<u64, AnyWindowHandle>,
    contexts: HashMap<u64, ContextBinding>,
    window_contexts: HashMap<BevyEntity, Vec<u64>>,
    cursor_positions: HashMap<BevyEntity, Vec2>,
    focused_contexts: HashMap<BevyEntity, u64>,
    modifiers: HashMap<BevyEntity, Modifiers>,
    capslock: HashMap<BevyEntity, Capslock>,
    clicks: HashMap<(BevyEntity, BevyMouseButton), ClickState>,
    application_active: bool,
}

#[derive(Clone, Copy)]
struct ContextBinding {
    window: Option<BevyEntity>,
    viewport: Rect,
    order: isize,
}

struct ClickState {
    at: Instant,
    position: Vec2,
    count: usize,
}

impl GpuiRuntime {
    #[cfg_attr(not(feature = "render"), allow(dead_code))]
    fn open_root<V, F>(&mut self, id: u64, options: WindowOptions, build: &F) -> Result<()>
    where
        V: Render + 'static,
        F: Fn(&mut Window, &mut App) -> Entity<V> + 'static,
    {
        self.platform.prepare_open_context(id)?;
        let window = self
            .application
            .update(|cx| cx.open_window(options, |window, cx| build(window, cx)))?
            .inspect_err(|_| self.platform.cancel_open_context(id))?;
        self.windows.insert(id, window.into());
        self.contexts.insert(
            id,
            ContextBinding {
                window: None,
                viewport: Rect::default(),
                order: 0,
            },
        );
        Ok(())
    }

    fn context_ids_at(&self, window: BevyEntity, position: Option<Vec2>) -> Vec<u64> {
        let position = position.or_else(|| self.cursor_positions.get(&window).copied());
        self.window_contexts
            .get(&window)
            .into_iter()
            .flatten()
            .copied()
            .filter(|id| {
                position.is_none_or(|position| {
                    self.contexts
                        .get(id)
                        .is_some_and(|binding| binding.viewport.contains(position))
                })
            })
            .collect()
    }

    fn keyboard_context_ids(&self, window: BevyEntity) -> Vec<u64> {
        if let Some(id) = self.focused_contexts.get(&window)
            && self
                .contexts
                .get(id)
                .is_some_and(|binding| binding.window == Some(window))
        {
            return vec![*id];
        }
        if let Some(id) = self.window_contexts.get(&window).and_then(|ids| {
            ids.iter()
                .rev()
                .find(|id| self.platform.accepts_text_input(**id))
        }) {
            return vec![*id];
        }
        self.window_contexts
            .get(&window)
            .and_then(|ids| ids.last())
            .copied()
            .into_iter()
            .collect()
    }

    fn close_context(&mut self, id: u64) -> Result<()> {
        if let Some(window) = self.windows.remove(&id) {
            self.application
                .update(|cx| window.update(cx, |_, window, _| window.remove_window()))??;
        }
        self.platform.remove_context(id);
        self.contexts.remove(&id);
        self.focused_contexts.retain(|_, focused| *focused != id);
        Ok(())
    }

    fn click_count(
        &mut self,
        window: BevyEntity,
        button: BevyMouseButton,
        state: ButtonState,
    ) -> usize {
        if state == ButtonState::Released {
            return self
                .clicks
                .get(&(window, button))
                .map_or(1, |click| click.count);
        }

        let now = Instant::now();
        let position = self
            .cursor_positions
            .get(&window)
            .copied()
            .unwrap_or_default();
        let click = self.clicks.entry((window, button)).or_insert(ClickState {
            at: now,
            position,
            count: 0,
        });
        click.count = if now.duration_since(click.at) <= Duration::from_millis(500)
            && click.position.distance(position) <= 4.0
        {
            click.count.saturating_add(1)
        } else {
            1
        };
        click.at = now;
        click.position = position;
        click.count
    }
}

/// Associates a retained GPUI root with a Bevy camera/render target.
///
/// The component is inserted by [`GpuiContexts::set_root`]. Keeping the
/// association in ECS lets ordinary Bevy camera, window, viewport, and entity
/// lifecycle changes remain the source of truth.
#[derive(Clone, Copy, Debug, Component)]
pub struct GpuiContext {
    id: u64,
}

/// Marks the camera/context used by [`GpuiContexts::primary`].
#[derive(Clone, Copy, Debug, Default, Component)]
pub struct PrimaryGpuiContext;

/// Typed token for a retained GPUI root owned by [`GpuiPlugin`](crate::GpuiPlugin).
pub struct GpuiViewHandle<V> {
    id: u64,
    view_type: PhantomData<fn() -> V>,
}

impl<V> Copy for GpuiViewHandle<V> {}

impl<V> Clone for GpuiViewHandle<V> {
    fn clone(&self) -> Self {
        *self
    }
}

/// Main-thread access to the embedded GPUI application.
#[derive(SystemParam)]
pub struct GpuiContexts<'w, 's> {
    state: NonSendMut<'w, GpuiRuntimeState>,
    #[cfg_attr(not(feature = "render"), allow(dead_code))]
    commands: Commands<'w, 's>,
    primary: Query<'w, 's, &'static GpuiContext, With<PrimaryGpuiContext>>,
}

impl GpuiContexts<'_, '_> {
    /// Returns a typed handle for the context marked [`PrimaryGpuiContext`].
    ///
    /// The view type is validated when the handle is used with [`Self::update`].
    pub fn primary<V>(&self) -> Result<GpuiViewHandle<V>>
    where
        V: Render + 'static,
    {
        let context = self
            .primary
            .single()
            .map_err(|error| anyhow!("expected exactly one PrimaryGpuiContext: {error}"))?;
        Ok(GpuiViewHandle {
            id: context.id,
            view_type: PhantomData,
        })
    }

    /// Creates a retained GPUI root associated with a Bevy camera.
    ///
    /// `camera` must be a live entity carrying the camera/render-target
    /// components queried by Bevy. This method queues component insertion but
    /// does not validate that precondition synchronously.
    ///
    /// Calls made before Bevy's render device is initialized are queued and
    /// materialized automatically once the shared GPUI atlas is ready. The
    /// builder must be reusable because it is invoked again if Bevy replaces a
    /// lost render device; the typed handle keeps the same ID after recovery.
    /// A deferred materialization failure is logged and leaves the handle
    /// pending, while a failure with an already-ready runtime is returned here.
    pub fn set_root<V, F>(&mut self, camera: BevyEntity, build: F) -> Result<GpuiViewHandle<V>>
    where
        V: Render + 'static,
        F: Fn(&mut Window, &mut App) -> Entity<V> + 'static,
    {
        self.set_root_with_options(camera, WindowOptions::default(), build)
    }

    /// Creates a retained GPUI root with explicit virtual-window options.
    ///
    /// Like [`Self::set_root`], `build` may run again during render-device
    /// recovery and should reconstruct its initial view from durable Bevy or
    /// shared application state. Deferred materialization failures are logged;
    /// synchronous failures are returned.
    pub fn set_root_with_options<V, F>(
        &mut self,
        camera: BevyEntity,
        options: WindowOptions,
        build: F,
    ) -> Result<GpuiViewHandle<V>>
    where
        V: Render + 'static,
        F: Fn(&mut Window, &mut App) -> Entity<V> + 'static,
    {
        #[cfg(not(feature = "render"))]
        {
            let _ = (camera, options, build);
            Err(anyhow!(
                "GpuiContexts::set_root requires bevy_gpui's `render` feature"
            ))
        }

        #[cfg(feature = "render")]
        {
            let id = self.state.next_id;
            self.state.next_id += 1;
            let create: RootFactory = Rc::new(move |runtime: &mut GpuiRuntime| {
                runtime.open_root(id, options.clone(), &build)
            });
            if let Some(runtime) = self.state.runtime.as_mut() {
                create(runtime)?;
            }
            self.state.root_factories.insert(id, create);
            self.commands.entity(camera).insert(GpuiContext { id });
            Ok(GpuiViewHandle {
                id,
                view_type: PhantomData,
            })
        }
    }

    /// Updates a retained root with typed access to its view and GPUI window.
    pub fn update<V, R>(
        &mut self,
        handle: &GpuiViewHandle<V>,
        update: impl FnOnce(&mut V, &mut Window, &mut Context<V>) -> R,
    ) -> Result<R>
    where
        V: Render + 'static,
    {
        let runtime = self
            .state
            .runtime
            .as_mut()
            .context("the embedded GPUI runtime is not ready yet")?;
        let window = runtime
            .windows
            .get(&handle.id)
            .copied()
            .ok_or_else(|| anyhow!("GPUI view handle is stale or still pending"))?;
        let window = window
            .downcast::<V>()
            .ok_or_else(|| anyhow!("GPUI view handle has the wrong view type"))?;
        runtime.application.update(|cx| window.update(cx, update))?
    }
}

#[cfg(feature = "render")]
struct AutoAttachedRoot;

#[cfg(feature = "render")]
impl Render for AutoAttachedRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
        div()
    }
}

#[cfg(feature = "render")]
pub(crate) fn auto_attach_primary(
    settings: Res<GpuiPluginSettings>,
    cameras: Query<(BevyEntity, &Camera, &RenderTarget, Option<&GpuiContext>)>,
    primary_window: Query<BevyEntity, With<PrimaryWindow>>,
    mut gpui: GpuiContexts,
) {
    if !settings.auto_attach_primary {
        return;
    }
    let primary_window = primary_window.single().ok();
    let mut selected = None;
    for (entity, camera, target, context) in &cameras {
        if !camera.is_active
            || !matches!(
                target.normalize(primary_window),
                Some(NormalizedRenderTarget::Window(window))
                    if Some(window.entity()) == primary_window
            )
        {
            continue;
        }
        if context.is_some() {
            return;
        }
        if selected.is_none_or(|(_, order)| camera.order >= order) {
            selected = Some((entity, camera.order));
        }
    }

    if let Some((camera, _)) = selected
        && let Err(error) = gpui.set_root(camera, |_, cx| cx.new(|_| AutoAttachedRoot))
    {
        bevy_log::warn!(?error, "failed to auto-attach GPUI to the primary camera");
    } else if let Some((camera, _)) = selected {
        gpui.commands.entity(camera).insert(PrimaryGpuiContext);
    }
}

#[cfg(feature = "render")]
pub(crate) fn initialize_runtime(
    bridge: Res<GpuiGpuBridge>,
    image_registry: Res<GpuiImageRegistry>,
    mut state: NonSendMut<GpuiRuntimeState>,
    mut status: ResMut<GpuiRuntimeStatus>,
    contexts: Query<&GpuiContext>,
    event_loop_proxy: Option<Res<EventLoopProxyWrapper>>,
) {
    let Some((gpu_generation, atlas)) = bridge.snapshot() else {
        return;
    };
    let replacing_device = state.runtime.is_some()
        && state
            .gpu_generation
            .is_some_and(|generation| generation != gpu_generation);
    if replacing_device {
        shutdown_runtime(state.runtime.take());
        status.ready = false;
        status.roots = 0;
        status.recoveries = status.recoveries.saturating_add(1);
        bevy_log::warn!(
            old_generation = state.gpu_generation,
            new_generation = gpu_generation,
            "rebuilding embedded GPUI runtime for replacement Bevy render device"
        );
    }

    if state.runtime.is_none() {
        let wake_event_loop = event_loop_proxy.map(|proxy| {
            let proxy = (**proxy).clone();
            Arc::new(move || {
                let _ = proxy.send_event(WinitUserEvent::WakeUp);
            }) as Arc<dyn Fn() + Send + Sync>
        });
        let platform = BevyPlatform::new(atlas, wake_event_loop);
        let bridge = state.bridge.clone();
        let image_registry = image_registry.clone();
        let application =
            match Application::new_inaccessible(platform.clone()).start_embedded(move |cx| {
                cx.set_global(BevyBridgeGlobal(bridge));
                cx.set_global(image_registry);
            }) {
                Ok(application) => application,
                Err(error) => {
                    bevy_log::error!(?error, "failed to start embedded GPUI runtime");
                    return;
                }
            };
        state.runtime = Some(GpuiRuntime {
            application,
            platform,
            windows: HashMap::new(),
            contexts: HashMap::new(),
            window_contexts: HashMap::new(),
            cursor_positions: HashMap::new(),
            focused_contexts: HashMap::new(),
            modifiers: HashMap::new(),
            capslock: HashMap::new(),
            clicks: HashMap::new(),
            application_active: true,
        });
        state.gpu_generation = Some(gpu_generation);
        status.ready = true;
        status.gpu_generation = gpu_generation;
        bevy_log::info!("embedded GPUI runtime initialized on Bevy's GPU");
    }

    let active_ids: HashSet<_> = contexts.iter().map(|context| context.id).collect();
    let to_materialize: Vec<_> = state
        .root_factories
        .iter()
        .filter(|(id, _)| active_ids.contains(id))
        .filter(|(id, _)| {
            state
                .runtime
                .as_ref()
                .is_some_and(|runtime| !runtime.contexts.contains_key(id))
        })
        .map(|(id, factory)| (*id, factory.clone()))
        .collect();
    let runtime = state.runtime.as_mut().expect("runtime was initialized");
    for (id, create) in to_materialize {
        if !active_ids.contains(&id) {
            continue;
        }
        if let Err(error) = create(runtime) {
            bevy_log::error!(?error, "failed to create queued GPUI root");
        } else {
            status.roots += 1;
            bevy_log::info!(roots = status.roots, "GPUI root attached to Bevy camera");
        }
    }
}

pub(crate) fn apply_deferred_bridge(world: &mut bevy_ecs::world::World) {
    let bridge = world
        .get_non_send::<GpuiRuntimeState>()
        .expect("GpuiPlugin runtime state is missing")
        .bridge
        .clone();
    bridge.drain_into(world);
}

pub(crate) fn drive_executor(
    mut state: NonSendMut<GpuiRuntimeState>,
    mut exits: Option<MessageWriter<AppExit>>,
) {
    if let Some(runtime) = state.runtime.as_mut() {
        runtime.platform.dispatcher().drain_main_queue(1024);
        if runtime.platform.quit_requested()
            && let Some(exits) = exits.as_mut()
        {
            exits.write(AppExit::Success);
        }
    }
}

#[derive(SystemParam)]
pub(crate) struct GpuiInputReaders<'w, 's> {
    cursors: Option<MessageReader<'w, 's, CursorMoved>>,
    cursor_entered: Option<MessageReader<'w, 's, CursorEntered>>,
    cursor_left: Option<MessageReader<'w, 's, CursorLeft>>,
    buttons: Option<MessageReader<'w, 's, MouseButtonInput>>,
    wheels: Option<MessageReader<'w, 's, MouseWheel>>,
    pinches: Option<MessageReader<'w, 's, PinchGesture>>,
    files: Option<MessageReader<'w, 's, FileDragAndDrop>>,
    window_focus: Option<MessageReader<'w, 's, WindowFocused>>,
    keys: Option<MessageReader<'w, 's, KeyboardInput>>,
    ime: Option<MessageReader<'w, 's, Ime>>,
    close_requested: Option<MessageReader<'w, 's, WindowCloseRequested>>,
    moved: Option<MessageReader<'w, 's, WindowMoved>>,
    theme_changed: Option<MessageReader<'w, 's, WindowThemeChanged>>,
    lifecycle: Option<MessageReader<'w, 's, AppLifecycle>>,
}

pub(crate) fn dispatch_input(
    mut input: GpuiInputReaders,
    mut runtime: NonSendMut<GpuiRuntimeState>,
    mut input_state: ResMut<GpuiInputState>,
) {
    input_state.wants_keyboard_input = false;
    input_state.default_prevented = false;
    let Some(runtime) = runtime.runtime.as_mut() else {
        input_state.pointer_claims.clear();
        input_state.wants_pointer_input = false;
        return;
    };
    if let Some(lifecycle) = input.lifecycle.as_mut() {
        for event in lifecycle.read() {
            runtime.application_active = event.is_active();
            for id in runtime.contexts.keys().copied() {
                runtime
                    .platform
                    .set_context_active(id, runtime.application_active);
            }
        }
    }
    if let Some(close_requested) = input.close_requested.as_mut() {
        for event in close_requested.read() {
            for id in runtime.context_ids_at(event.window, None) {
                if !runtime.platform.should_close(id) {
                    bevy_log::warn!(
                        id,
                        window = ?event.window,
                        "GPUI requested a close veto, but Bevy's default WindowPlugin owns close policy"
                    );
                }
            }
        }
    }
    if let Some(moved) = input.moved.as_mut() {
        for event in moved.read() {
            for id in runtime.context_ids_at(event.window, None) {
                runtime.platform.notify_window_moved(id);
            }
        }
    }
    if let Some(theme_changed) = input.theme_changed.as_mut() {
        for event in theme_changed.read() {
            let appearance = map_window_theme(event.theme);
            for id in runtime.context_ids_at(event.window, None) {
                runtime.platform.set_window_appearance(id, appearance);
            }
        }
    }
    if let Some(cursors) = input.cursors.as_mut() {
        for event in cursors.read() {
            input_state.pointer_claims.remove(&event.window);
            runtime
                .cursor_positions
                .insert(event.window, event.position);
            for id in runtime.context_ids_at(event.window, Some(event.position)) {
                let binding = runtime.contexts[&id];
                let local = event.position - binding.viewport.min;
                let modifiers = runtime
                    .modifiers
                    .get(&event.window)
                    .copied()
                    .unwrap_or_default();
                record_pointer_dispatch(
                    event.window,
                    runtime.platform.dispatch_input(
                        id,
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position: point(px(local.x), px(local.y)),
                            pressed_button: runtime.platform.pressed_button(id),
                            modifiers,
                        }),
                    ),
                    &mut input_state,
                );
            }
        }
    }

    if let Some(cursor_entered) = input.cursor_entered.as_mut() {
        for event in cursor_entered.read() {
            for id in runtime.context_ids_at(event.window, None) {
                runtime.platform.set_hovered(id, true);
            }
        }
    }

    if let Some(cursor_left) = input.cursor_left.as_mut() {
        for event in cursor_left.read() {
            let modifiers = runtime
                .modifiers
                .get(&event.window)
                .copied()
                .unwrap_or_default();
            for id in runtime.context_ids_at(event.window, None) {
                runtime.platform.set_hovered(id, false);
                record_pointer_dispatch(
                    event.window,
                    runtime.platform.dispatch_input(
                        id,
                        PlatformInput::MouseExited(MouseExitEvent {
                            position: runtime.platform.mouse_position(id),
                            pressed_button: None,
                            modifiers,
                        }),
                    ),
                    &mut input_state,
                );
            }
            runtime.cursor_positions.remove(&event.window);
            input_state.pointer_claims.remove(&event.window);
        }
    }

    if let Some(window_focus) = input.window_focus.as_mut() {
        for event in window_focus.read() {
            if event.focused {
                continue;
            }
            runtime.focused_contexts.remove(&event.window);
            runtime.modifiers.remove(&event.window);
            runtime.capslock.remove(&event.window);
            for id in runtime.context_ids_at(event.window, None) {
                runtime
                    .platform
                    .set_keyboard_state(id, Modifiers::default(), Capslock::default());
                record_keyboard_dispatch(
                    runtime.platform.dispatch_input(
                        id,
                        PlatformInput::ModifiersChanged(ModifiersChangedEvent::default()),
                    ),
                    &mut input_state,
                );
            }
        }
    }

    if let Some(buttons) = input.buttons.as_mut() {
        for event in buttons.read() {
            let Some(button) = map_mouse_button(event.button) else {
                continue;
            };
            input_state.pointer_claims.remove(&event.window);
            let ids = runtime.context_ids_at(event.window, None);
            if event.state == ButtonState::Pressed
                && let Some(id) = ids.last().copied()
            {
                runtime.focused_contexts.insert(event.window, id);
            }
            let modifiers = runtime
                .modifiers
                .get(&event.window)
                .copied()
                .unwrap_or_default();
            let click_count = runtime.click_count(event.window, event.button, event.state);
            for id in ids {
                let position = runtime.platform.mouse_position(id);
                let input = match event.state {
                    ButtonState::Pressed => PlatformInput::MouseDown(MouseDownEvent {
                        button,
                        position,
                        modifiers,
                        click_count,
                        first_mouse: false,
                    }),
                    ButtonState::Released => PlatformInput::MouseUp(MouseUpEvent {
                        button,
                        position,
                        modifiers,
                        click_count,
                    }),
                };
                record_pointer_dispatch(
                    event.window,
                    runtime.platform.dispatch_input(id, input),
                    &mut input_state,
                );
            }
        }
    }

    if let Some(wheels) = input.wheels.as_mut() {
        for event in wheels.read() {
            input_state.pointer_claims.remove(&event.window);
            let delta = match event.unit {
                MouseScrollUnit::Line => ScrollDelta::Lines(point(event.x, event.y)),
                MouseScrollUnit::Pixel => ScrollDelta::Pixels(point(px(event.x), px(event.y))),
            };
            let phase = match event.phase {
                BevyTouchPhase::Started => TouchPhase::Started,
                BevyTouchPhase::Moved => TouchPhase::Moved,
                BevyTouchPhase::Ended | BevyTouchPhase::Canceled => TouchPhase::Ended,
            };
            let modifiers = runtime
                .modifiers
                .get(&event.window)
                .copied()
                .unwrap_or_default();
            for id in runtime.context_ids_at(event.window, None) {
                record_pointer_dispatch(
                    event.window,
                    runtime.platform.dispatch_input(
                        id,
                        PlatformInput::ScrollWheel(ScrollWheelEvent {
                            position: runtime.platform.mouse_position(id),
                            delta,
                            modifiers,
                            touch_phase: phase,
                        }),
                    ),
                    &mut input_state,
                );
            }
        }
    }

    if let Some(pinches) = input.pinches.as_mut() {
        for event in pinches.read() {
            let focused: Vec<_> = runtime.focused_contexts.values().copied().collect();
            let ids = if focused.is_empty() {
                runtime
                    .window_contexts
                    .values()
                    .filter_map(|ids| ids.last().copied())
                    .take(1)
                    .collect()
            } else {
                focused
            };
            for id in ids {
                let window = runtime.contexts.get(&id).and_then(|binding| binding.window);
                let modifiers = window
                    .and_then(|window| runtime.modifiers.get(&window).copied())
                    .unwrap_or_default();
                let result = runtime.platform.dispatch_input(
                    id,
                    PlatformInput::Pinch(PinchEvent {
                        position: runtime.platform.mouse_position(id),
                        delta: event.0,
                        modifiers,
                        phase: TouchPhase::Moved,
                    }),
                );
                if let Some(window) = window {
                    record_pointer_dispatch(window, result, &mut input_state);
                }
            }
        }
    }

    if let Some(files) = input.files.as_mut() {
        for event in files.read() {
            let window = match event {
                FileDragAndDrop::DroppedFile { window, .. }
                | FileDragAndDrop::HoveredFile { window, .. }
                | FileDragAndDrop::HoveredFileCanceled { window } => *window,
            };
            input_state.pointer_claims.remove(&window);
            for id in runtime.context_ids_at(window, None) {
                let position = runtime.platform.mouse_position(id);
                let input = match event {
                    FileDragAndDrop::HoveredFile { path_buf, .. } => {
                        Some(PlatformInput::FileDrop(FileDropEvent::Entered {
                            position,
                            paths: ExternalPaths(vec![path_buf.clone()].into()),
                        }))
                    }
                    FileDragAndDrop::DroppedFile { path_buf, .. } => {
                        record_pointer_dispatch(
                            window,
                            runtime.platform.dispatch_input(
                                id,
                                PlatformInput::FileDrop(FileDropEvent::Entered {
                                    position,
                                    paths: ExternalPaths(vec![path_buf.clone()].into()),
                                }),
                            ),
                            &mut input_state,
                        );
                        Some(PlatformInput::FileDrop(FileDropEvent::Submit { position }))
                    }
                    FileDragAndDrop::HoveredFileCanceled { .. } => {
                        Some(PlatformInput::FileDrop(FileDropEvent::Exited))
                    }
                };
                if let Some(input) = input {
                    record_pointer_dispatch(
                        window,
                        runtime.platform.dispatch_input(id, input),
                        &mut input_state,
                    );
                }
            }
        }
    }

    if let Some(keys) = input.keys.as_mut() {
        for event in keys.read() {
            let is_modifier = update_modifier_state(
                &event.logical_key,
                event.state,
                runtime.modifiers.entry(event.window).or_default(),
                runtime.capslock.entry(event.window).or_default(),
            );
            let modifiers = runtime.modifiers[&event.window];
            let capslock = runtime.capslock[&event.window];
            let ids = runtime.keyboard_context_ids(event.window);

            if is_modifier {
                for id in ids {
                    runtime.platform.set_keyboard_state(id, modifiers, capslock);
                    record_keyboard_dispatch(
                        runtime.platform.dispatch_input(
                            id,
                            PlatformInput::ModifiersChanged(ModifiersChangedEvent {
                                modifiers,
                                capslock,
                            }),
                        ),
                        &mut input_state,
                    );
                }
                continue;
            }

            let Some(key) = map_logical_key(&event.logical_key) else {
                continue;
            };
            let key_char = event.text.as_ref().map(ToString::to_string).or_else(|| {
                if let BevyKey::Character(character) = &event.logical_key {
                    Some(character.to_string())
                } else {
                    None
                }
            });
            for id in ids {
                runtime.platform.set_keyboard_state(id, modifiers, capslock);
                let prefer_character_input = runtime.platform.prefers_ime_for_printable_keys(id);
                let keystroke = Keystroke {
                    modifiers,
                    key: key.clone(),
                    key_char: key_char.clone(),
                    physical_key: Some(format!("{:?}", event.key_code).to_ascii_lowercase()),
                };
                let input = match event.state {
                    ButtonState::Pressed => PlatformInput::KeyDown(KeyDownEvent {
                        keystroke,
                        is_held: event.repeat,
                        prefer_character_input,
                    }),
                    ButtonState::Released => PlatformInput::KeyUp(KeyUpEvent { keystroke }),
                };
                let result = runtime.platform.dispatch_input(id, input);
                let insert_text = event.state == ButtonState::Pressed
                    && result.propagate
                    && !modifiers.platform
                    && (!modifiers.control || modifiers.alt);
                record_keyboard_dispatch(result, &mut input_state);
                if insert_text
                    && let Some(text) = event.text.as_deref()
                    && runtime.platform.ime_commit(id, text)
                {
                    input_state.wants_keyboard_input = true;
                }
            }
        }
    }

    if let Some(ime) = input.ime.as_mut() {
        for event in ime.read() {
            let window = match event {
                Ime::Preedit { window, .. }
                | Ime::Commit { window, .. }
                | Ime::Enabled { window }
                | Ime::Disabled { window } => *window,
            };
            for id in runtime.keyboard_context_ids(window) {
                let handled = match event {
                    Ime::Preedit { value, cursor, .. } => runtime.platform.ime_preedit(
                        id,
                        value,
                        cursor.and_then(|(start, end)| byte_range_to_utf16(value, start, end)),
                    ),
                    Ime::Commit { value, .. } => runtime.platform.ime_commit(id, value),
                    Ime::Enabled { .. } => runtime.platform.accepts_text_input(id),
                    Ime::Disabled { .. } => {
                        runtime.platform.ime_cancel(id);
                        false
                    }
                };
                input_state.wants_keyboard_input |= handled;
            }
        }
    }

    input_state.wants_keyboard_input |= runtime
        .window_contexts
        .values()
        .flatten()
        .copied()
        .any(|id| runtime.platform.accepts_text_input(id));
    input_state
        .pointer_claims
        .retain(|window| runtime.window_contexts.contains_key(window));
    input_state.wants_pointer_input = !input_state.pointer_claims.is_empty();
}

fn record_pointer_dispatch(
    window: BevyEntity,
    result: gpui::DispatchEventResult,
    state: &mut GpuiInputState,
) {
    state.default_prevented |= result.default_prevented;
    if result.pointer_hit || !result.propagate || result.default_prevented {
        state.pointer_claims.insert(window);
    }
}

fn record_keyboard_dispatch(result: gpui::DispatchEventResult, state: &mut GpuiInputState) {
    state.default_prevented |= result.default_prevented;
    state.wants_keyboard_input |= !result.propagate || result.default_prevented;
}

fn update_modifier_state(
    key: &BevyKey,
    state: ButtonState,
    modifiers: &mut Modifiers,
    capslock: &mut Capslock,
) -> bool {
    let pressed = state == ButtonState::Pressed;
    match key {
        BevyKey::Alt => modifiers.alt = pressed,
        BevyKey::AltGraph => {
            modifiers.alt = pressed;
            modifiers.control = pressed;
        }
        BevyKey::Control => modifiers.control = pressed,
        BevyKey::Shift => modifiers.shift = pressed,
        BevyKey::Super | BevyKey::Meta | BevyKey::Hyper => modifiers.platform = pressed,
        BevyKey::Fn => modifiers.function = pressed,
        BevyKey::CapsLock if pressed => capslock.on = !capslock.on,
        BevyKey::CapsLock => {}
        _ => return false,
    }
    true
}

fn map_logical_key(key: &BevyKey) -> Option<String> {
    let name = match key {
        BevyKey::Character(character) => return Some(character.to_ascii_lowercase()),
        BevyKey::Dead(Some(character)) => return Some(character.to_string()),
        BevyKey::Dead(None) | BevyKey::Unidentified(_) => return None,
        BevyKey::Enter => "enter",
        BevyKey::Tab => "tab",
        BevyKey::Space => "space",
        BevyKey::ArrowDown => "down",
        BevyKey::ArrowLeft => "left",
        BevyKey::ArrowRight => "right",
        BevyKey::ArrowUp => "up",
        BevyKey::End => "end",
        BevyKey::Home => "home",
        BevyKey::PageDown => "pagedown",
        BevyKey::PageUp => "pageup",
        BevyKey::Backspace => "backspace",
        BevyKey::Delete => "delete",
        BevyKey::Insert => "insert",
        BevyKey::Escape => "escape",
        BevyKey::F1 => "f1",
        BevyKey::F2 => "f2",
        BevyKey::F3 => "f3",
        BevyKey::F4 => "f4",
        BevyKey::F5 => "f5",
        BevyKey::F6 => "f6",
        BevyKey::F7 => "f7",
        BevyKey::F8 => "f8",
        BevyKey::F9 => "f9",
        BevyKey::F10 => "f10",
        BevyKey::F11 => "f11",
        BevyKey::F12 => "f12",
        BevyKey::F13 => "f13",
        BevyKey::F14 => "f14",
        BevyKey::F15 => "f15",
        BevyKey::F16 => "f16",
        BevyKey::F17 => "f17",
        BevyKey::F18 => "f18",
        BevyKey::F19 => "f19",
        BevyKey::F20 => "f20",
        BevyKey::F21 => "f21",
        BevyKey::F22 => "f22",
        BevyKey::F23 => "f23",
        BevyKey::F24 => "f24",
        BevyKey::F25 => "f25",
        BevyKey::F26 => "f26",
        BevyKey::F27 => "f27",
        BevyKey::F28 => "f28",
        BevyKey::F29 => "f29",
        BevyKey::F30 => "f30",
        BevyKey::F31 => "f31",
        BevyKey::F32 => "f32",
        BevyKey::F33 => "f33",
        BevyKey::F34 => "f34",
        BevyKey::F35 => "f35",
        _ => return Some(format!("{key:?}").to_ascii_lowercase()),
    };
    Some(name.to_owned())
}

fn byte_range_to_utf16(text: &str, start: usize, end: usize) -> Option<std::ops::Range<usize>> {
    if start > end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        return None;
    }
    Some(text[..start].encode_utf16().count()..text[..end].encode_utf16().count())
}

fn map_mouse_button(button: BevyMouseButton) -> Option<MouseButton> {
    match button {
        BevyMouseButton::Left => Some(MouseButton::Left),
        BevyMouseButton::Right => Some(MouseButton::Right),
        BevyMouseButton::Middle => Some(MouseButton::Middle),
        BevyMouseButton::Back => Some(MouseButton::Navigate(NavigationDirection::Back)),
        BevyMouseButton::Forward => Some(MouseButton::Navigate(NavigationDirection::Forward)),
        BevyMouseButton::Other(_) => None,
    }
}

fn map_window_theme(theme: WindowTheme) -> gpui::WindowAppearance {
    match theme {
        WindowTheme::Light => gpui::WindowAppearance::Light,
        WindowTheme::Dark => gpui::WindowAppearance::Dark,
    }
}

pub(crate) fn prepare_window_close(
    mut close_requested: Option<MessageReader<WindowCloseRequested>>,
    mut contexts: Query<(&mut Camera, &RenderTarget), With<GpuiContext>>,
    primary_window: Query<BevyEntity, With<PrimaryWindow>>,
) {
    let Some(close_requested) = close_requested.as_mut() else {
        return;
    };
    let primary_window = primary_window.single().ok();
    for event in close_requested.read() {
        for (mut camera, target) in &mut contexts {
            if matches!(
                target.normalize(primary_window),
                Some(NormalizedRenderTarget::Window(window))
                    if window.entity() == event.window
            ) {
                // Bevy's renderer may still consume the preceding extracted
                // frame while WindowPlugin enters its one-frame closing state.
                camera.is_active = false;
            }
        }
    }
}

pub(crate) fn cleanup_orphaned_window_contexts(
    mut commands: Commands,
    mut state: NonSendMut<GpuiRuntimeState>,
    mut status: ResMut<GpuiRuntimeStatus>,
    mut contexts: Query<(BevyEntity, &GpuiContext, &mut Camera, &RenderTarget)>,
    windows: Query<(), With<BevyWindow>>,
    primary_window: Query<BevyEntity, With<PrimaryWindow>>,
) {
    let primary_window = primary_window.single().ok();
    for (camera_entity, context, mut camera, target) in &mut contexts {
        let Some(NormalizedRenderTarget::Window(window)) = target.normalize(primary_window) else {
            continue;
        };
        if windows.contains(window.entity()) {
            continue;
        }

        // Bevy treats a missing window target as a camera error. Disable the
        // orphan before CameraUpdateSystems and leave retargeting/re-enabling to
        // the application that owns the camera.
        camera.is_active = false;
        if let Some(runtime) = state.runtime.as_mut()
            && let Err(error) = runtime.close_context(context.id)
        {
            bevy_log::warn!(
                ?error,
                id = context.id,
                "failed to close GPUI root for removed Bevy window"
            );
        }
        state.root_factories.remove(&context.id);
        status.roots = status.roots.saturating_sub(1);
        commands
            .entity(camera_entity)
            .remove::<(GpuiContext, PrimaryGpuiContext, GpuiScene)>();
    }
}

pub(crate) fn sync_windows(
    mut commands: Commands,
    mut state: NonSendMut<GpuiRuntimeState>,
    mut status: ResMut<GpuiRuntimeStatus>,
    queries: GpuiWindowQueries,
) {
    let GpuiWindowQueries {
        contexts,
        stale_scenes,
        mut windows,
        primary_window,
        monitors,
        primary_monitor,
    } = queries;
    let Some(runtime) = state.runtime.as_mut() else {
        status.roots = 0;
        return;
    };
    let primary_window = primary_window.single().ok();
    let primary_monitor = primary_monitor.single().ok();
    let mut live_monitors = Vec::new();
    for (entity, monitor) in &monitors {
        let id = entity.to_bits();
        live_monitors.push(id);
        runtime.platform.sync_display(
            id,
            monitor.physical_position,
            monitor.physical_size(),
            monitor.scale_factor,
            Some(entity) == primary_monitor,
        );
    }
    runtime.platform.retain_displays(&live_monitors);
    runtime.window_contexts.clear();
    for entity in &stale_scenes {
        commands.entity(entity).remove::<GpuiScene>();
    }

    let live_ids: HashSet<_> = contexts
        .iter()
        .map(|(_, context, _, _)| context.id)
        .collect();
    let stale_ids: Vec<_> = runtime
        .contexts
        .keys()
        .copied()
        .filter(|id| !live_ids.contains(id))
        .collect();
    for &id in &stale_ids {
        if let Err(error) = runtime.close_context(id) {
            bevy_log::warn!(?error, id, "failed to close removed GPUI context cleanly");
        }
        status.roots = status.roots.saturating_sub(1);
    }

    for (_, context, camera, target) in &contexts {
        let Some(binding) = runtime.contexts.get_mut(&context.id) else {
            continue;
        };
        let viewport = camera.logical_viewport_rect().unwrap_or_default();
        binding.viewport = viewport;
        binding.order = camera.order;
        binding.window = match target.normalize(primary_window) {
            Some(NormalizedRenderTarget::Window(window)) => Some(window.entity()),
            _ => None,
        };

        if let Some(window_entity) = binding.window {
            runtime
                .window_contexts
                .entry(window_entity)
                .or_default()
                .push(context.id);
            if let Ok((window, _, _, on_monitor, raw_handles)) = windows.get(window_entity) {
                if let Some(on_monitor) = on_monitor {
                    runtime
                        .platform
                        .set_window_display(context.id, on_monitor.0.to_bits());
                }
                if let Some(raw_handles) = raw_handles {
                    runtime.platform.set_window_raw_handles(
                        context.id,
                        raw_handles.get_window_handle(),
                        raw_handles.get_display_handle(),
                    );
                }
                if let Some(theme) = window.window_theme {
                    runtime
                        .platform
                        .set_window_appearance(context.id, map_window_theme(theme));
                }
                runtime.platform.sync_window(
                    context.id,
                    viewport,
                    window.scale_factor(),
                    window.focused && runtime.application_active,
                );
            }
        } else {
            runtime.platform.sync_window(
                context.id,
                viewport,
                camera.target_scaling_factor().unwrap_or(1.0),
                true,
            );
        }
    }

    for ids in runtime.window_contexts.values_mut() {
        ids.sort_by_key(|id| runtime.contexts.get(id).map_or(0, |binding| binding.order));
    }

    let mapped_windows: Vec<_> = runtime
        .window_contexts
        .iter()
        .map(|(window, ids)| (*window, ids.clone()))
        .collect();
    for (window_entity, ids) in mapped_windows {
        let Ok((mut window, mut cursor_options, current_cursor, _, _)) =
            windows.get_mut(window_entity)
        else {
            continue;
        };
        let cursor = CursorIcon::System(map_cursor_style(
            ids.last().copied().map_or(gpui::CursorStyle::Arrow, |id| {
                runtime.platform.cursor_style(id)
            }),
        ));
        let wants_ime = ids
            .iter()
            .copied()
            .any(|id| runtime.platform.accepts_text_input(id));
        window.ime_enabled = wants_ime;

        if current_cursor != Some(&cursor) {
            commands.entity(window_entity).insert(cursor.clone());
        }

        for id in ids {
            let output = runtime.platform.take_window_output(id);
            if let Some(title) = output.title {
                window.title = title;
            }
            if let Some(size) = output.logical_size {
                window
                    .resolution
                    .set(f32::from(size.width), f32::from(size.height));
            }
            if let Some(fullscreen) = output.fullscreen {
                window.mode = if fullscreen {
                    WindowMode::BorderlessFullscreen(MonitorSelection::Current)
                } else {
                    WindowMode::Windowed
                };
            }
            if output.minimize {
                window.set_minimized(true);
            }
            if output.maximize {
                window.set_maximized(true);
            }
            if output.activate {
                window.focused = true;
            }
            if let Some(position) = output.ime_position {
                window.ime_position = Vec2::new(position.x.into(), position.y.into());
            }
            if let Some(visible) = output.cursor_visible {
                cursor_options.visible = visible;
            }
            if let Some(appearance) = output.background_appearance {
                window.transparent = appearance != gpui::WindowBackgroundAppearance::Opaque;
                if matches!(
                    appearance,
                    gpui::WindowBackgroundAppearance::Blurred
                        | gpui::WindowBackgroundAppearance::MicaBackdrop
                        | gpui::WindowBackgroundAppearance::MicaAltBackdrop
                ) {
                    bevy_log::warn!(
                        ?appearance,
                        "Bevy window backend supports transparency but not this GPUI backdrop material"
                    );
                }
            }
        }
    }

    status.roots = runtime.contexts.len();
    for id in stale_ids {
        state.root_factories.remove(&id);
    }
}

#[derive(SystemParam)]
pub(crate) struct GpuiWindowQueries<'w, 's> {
    contexts: Query<
        'w,
        's,
        (
            BevyEntity,
            &'static GpuiContext,
            &'static Camera,
            &'static RenderTarget,
        ),
    >,
    stale_scenes: Query<'w, 's, BevyEntity, (With<GpuiScene>, Without<GpuiContext>)>,
    windows: Query<'w, 's, WindowSyncItem>,
    primary_window: Query<'w, 's, BevyEntity, With<PrimaryWindow>>,
    monitors: Query<'w, 's, (BevyEntity, &'static BevyMonitor)>,
    primary_monitor: Query<'w, 's, BevyEntity, With<PrimaryMonitor>>,
}

type WindowSyncItem = (
    &'static mut BevyWindow,
    &'static mut CursorOptions,
    Option<&'static CursorIcon>,
    Option<&'static OnMonitor>,
    Option<&'static RawHandleWrapper>,
);

fn map_cursor_style(style: gpui::CursorStyle) -> SystemCursorIcon {
    match style {
        gpui::CursorStyle::Arrow => SystemCursorIcon::Default,
        gpui::CursorStyle::IBeam => SystemCursorIcon::Text,
        gpui::CursorStyle::Crosshair => SystemCursorIcon::Crosshair,
        gpui::CursorStyle::ClosedHand => SystemCursorIcon::Grabbing,
        gpui::CursorStyle::OpenHand => SystemCursorIcon::Grab,
        gpui::CursorStyle::PointingHand => SystemCursorIcon::Pointer,
        gpui::CursorStyle::ResizeLeft => SystemCursorIcon::WResize,
        gpui::CursorStyle::ResizeRight => SystemCursorIcon::EResize,
        gpui::CursorStyle::ResizeLeftRight => SystemCursorIcon::EwResize,
        gpui::CursorStyle::ResizeUp => SystemCursorIcon::NResize,
        gpui::CursorStyle::ResizeDown => SystemCursorIcon::SResize,
        gpui::CursorStyle::ResizeUpDown => SystemCursorIcon::NsResize,
        gpui::CursorStyle::ResizeUpLeftDownRight => SystemCursorIcon::NwseResize,
        gpui::CursorStyle::ResizeUpRightDownLeft => SystemCursorIcon::NeswResize,
        gpui::CursorStyle::ResizeColumn => SystemCursorIcon::ColResize,
        gpui::CursorStyle::ResizeRow => SystemCursorIcon::RowResize,
        gpui::CursorStyle::IBeamCursorForVerticalLayout => SystemCursorIcon::VerticalText,
        gpui::CursorStyle::OperationNotAllowed => SystemCursorIcon::NotAllowed,
        gpui::CursorStyle::DragLink => SystemCursorIcon::Alias,
        gpui::CursorStyle::DragCopy => SystemCursorIcon::Copy,
        gpui::CursorStyle::ContextualMenu => SystemCursorIcon::ContextMenu,
    }
}

pub(crate) fn build_scenes(
    mut commands: Commands,
    mut state: NonSendMut<GpuiRuntimeState>,
    mut status: ResMut<GpuiRuntimeStatus>,
    contexts: Query<(BevyEntity, &GpuiContext)>,
    #[cfg(feature = "render")] image_registry: Res<GpuiImageRegistry>,
) {
    let Some(runtime) = state.runtime.as_mut() else {
        return;
    };
    #[cfg(feature = "render")]
    let retry_missing_images = image_registry.take_missing();
    #[cfg(not(feature = "render"))]
    let retry_missing_images = false;
    for (camera, context) in &contexts {
        let needs_frame = runtime
            .windows
            .get(&context.id)
            .copied()
            .and_then(|window| runtime.application.window_needs_frame(window).ok())
            .unwrap_or(false);
        if needs_frame || retry_missing_images {
            runtime.platform.request_frame(context.id);
        }
        if let Some((generation, snapshot)) = runtime.platform.take_latest_snapshot(context.id)
            && let Ok(mut entity) = commands.get_entity(camera)
        {
            entity.insert(GpuiScene::new(snapshot, generation));
            status.scenes_built += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_hit_claims_input_even_when_gpui_event_propagates() {
        let window = BevyEntity::from_raw_u32(7).expect("test entity should be valid");
        let mut state = GpuiInputState::default();

        record_pointer_dispatch(
            window,
            gpui::DispatchEventResult {
                propagate: true,
                default_prevented: false,
                pointer_hit: true,
            },
            &mut state,
        );

        assert!(state.pointer_claims.contains(&window));
    }

    #[test]
    fn unhandled_pointer_event_does_not_claim_input() {
        let window = BevyEntity::from_raw_u32(8).expect("test entity should be valid");
        let mut state = GpuiInputState::default();

        record_pointer_dispatch(
            window,
            gpui::DispatchEventResult {
                propagate: true,
                default_prevented: false,
                pointer_hit: false,
            },
            &mut state,
        );

        assert!(!state.pointer_claims.contains(&window));
    }

    #[test]
    fn maps_gpui_keybinding_names() {
        assert_eq!(map_logical_key(&BevyKey::Enter).as_deref(), Some("enter"));
        assert_eq!(
            map_logical_key(&BevyKey::ArrowLeft).as_deref(),
            Some("left")
        );
        assert_eq!(map_logical_key(&BevyKey::PageUp).as_deref(), Some("pageup"));
        assert_eq!(map_logical_key(&BevyKey::F35).as_deref(), Some("f35"));
        assert_eq!(
            map_logical_key(&BevyKey::Character("A".into())).as_deref(),
            Some("a")
        );
        assert_eq!(
            map_logical_key(&BevyKey::Unidentified(
                bevy_input::keyboard::NativeKey::Unidentified,
            )),
            None
        );
    }

    #[test]
    fn modifier_state_tracks_press_release_and_capslock_toggle() {
        let mut modifiers = Modifiers::default();
        let mut capslock = Capslock::default();

        assert!(update_modifier_state(
            &BevyKey::Shift,
            ButtonState::Pressed,
            &mut modifiers,
            &mut capslock,
        ));
        assert!(modifiers.shift);
        update_modifier_state(
            &BevyKey::Shift,
            ButtonState::Released,
            &mut modifiers,
            &mut capslock,
        );
        assert!(!modifiers.shift);

        update_modifier_state(
            &BevyKey::CapsLock,
            ButtonState::Pressed,
            &mut modifiers,
            &mut capslock,
        );
        assert!(capslock.on);
        update_modifier_state(
            &BevyKey::CapsLock,
            ButtonState::Released,
            &mut modifiers,
            &mut capslock,
        );
        assert!(capslock.on);
    }

    #[test]
    fn converts_ime_byte_offsets_to_utf16_offsets() {
        assert_eq!(byte_range_to_utf16("a😀b", 1, 5), Some(1..3));
        assert_eq!(byte_range_to_utf16("a😀b", 2, 5), None);
        assert_eq!(byte_range_to_utf16("abc", 3, 2), None);
    }

    #[test]
    fn maps_gpui_cursor_styles_to_bevy_system_cursors() {
        assert_eq!(
            map_cursor_style(gpui::CursorStyle::PointingHand),
            SystemCursorIcon::Pointer
        );
        assert_eq!(
            map_cursor_style(gpui::CursorStyle::ResizeLeftRight),
            SystemCursorIcon::EwResize
        );
        assert_eq!(
            map_cursor_style(gpui::CursorStyle::OperationNotAllowed),
            SystemCursorIcon::NotAllowed
        );
    }
}
