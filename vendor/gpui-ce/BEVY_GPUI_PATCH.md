# `bevy_gpui` GPUI fork provenance

The vendored source is based on
[`gpui-ce/gpui-ce@20340e14874a3b55122e5cb2aa0d023874e08b2d`](https://github.com/gpui-ce/gpui-ce/tree/20340e14874a3b55122e5cb2aa0d023874e08b2d).
`vendor/gpui-ce` is a pruned source copy, not a nested Git repository.

## Why it is vendored

The Bevy integration needs host-neutral APIs that are absent from the base
revision:

- `Application::start_embedded` and `EmbeddedApplication` for an explicitly
  driven, non-blocking application lifecycle;
- a `Send + Sync` `SceneSnapshot` that preserves render batch order;
- host-neutral external-surface IDs for Bevy-owned textures;
- `SceneRenderer::render` into a caller-owned WGPU command encoder and texture
  view, including viewport/scissor, HDR formats, and full-target filters;
- physical-key identity in `Keystroke`;
- `DispatchEventResult::pointer_hit`, including pointer-capture state, so the
  Bevy adapter can report UI ownership even when GPUI propagation remains true.

The external-target path resolves the same crates.io WGPU package identity as
Bevy 0.19. It neither creates nor presents a WGPU surface, and it never submits
the caller's command encoder.

## Changed-file inventory

The following files differ in content from the recorded upstream revision:

```text
Cargo.toml
Cargo.lock
crates/gpui/Cargo.toml
crates/gpui/src/app.rs
crates/gpui/src/platform.rs
crates/gpui/src/scene.rs
crates/gpui/src/window.rs
crates/gpui/src/app/visual_test_context.rs
crates/gpui/src/platform/keystroke.rs
crates/gpui_wgpu/src/gpui_wgpu.rs
crates/gpui_wgpu/src/wgpu_renderer.rs
```

`BEVY_GPUI_PATCH.md` itself is local provenance metadata. Other absent upstream
directories are deliberate pruning, not integration patches.

## Patch groups

| Patch group | Primary files | Required behavior |
|---|---|---|
| Embedded application lifetime | `crates/gpui/src/app.rs` | Explicit start, update, reentrancy failure, executor access, and shutdown without `Platform::run` |
| Sendable scene transfer | `crates/gpui/src/scene.rs`, `window.rs`, `visual_test_context.rs` | Immutable `SceneSnapshot`, preserved batch/filter order, and no platform window/surface objects |
| Pointer ownership | `crates/gpui/src/window.rs` | Dispatch result reports hitbox or capture ownership independently from propagation |
| Physical keyboard identity | `crates/gpui/src/platform/keystroke.rs` | `Keystroke` retains the physical key supplied by Bevy |
| External render targets | `crates/gpui_wgpu/src/gpui_wgpu.rs`, `wgpu_renderer.rs`, GPUI platform/scene files | Caller-owned encoder/view, load compositing, external textures, target formats, viewport/scissor, alpha, and full-target filters |
| Dependency unification | workspace and crate manifests/lockfile | One crates.io WGPU type identity shared with Bevy |

## Reconstruct the comparison

Clone the recorded base, compare a changed file, and audit the pruned tree inside
one disposable subshell:

```bash
(
  upstream=$(mktemp -d)
  trap 'rm -rf "$upstream"' EXIT
  git clone https://github.com/gpui-ce/gpui-ce.git "$upstream"
  git -C "$upstream" checkout 20340e14874a3b55122e5cb2aa0d023874e08b2d
  git diff --no-index -- \
    "$upstream/crates/gpui/src/window.rs" \
    vendor/gpui-ce/crates/gpui/src/window.rs || {
      rc=$?
      [ "$rc" -eq 1 ] || exit "$rc"
    }
  rsync -rcn --delete --itemize-changes \
    --exclude=.git --exclude=target \
    "$upstream/" vendor/gpui-ce/
)
```

Add a `git diff --no-index` line for every file in the inventory before leaving
the subshell. In the `rsync` dry-run output, content-changing files carry
checksum markers; files present only upstream are the deliberately pruned
surface.

## Required regression gates

```bash
cargo test --manifest-path vendor/gpui-ce/Cargo.toml -p gpui --lib
cargo test --manifest-path vendor/gpui-ce/Cargo.toml -p gpui_wgpu --lib
```

Keep the normal GPUI surface renderer working when changing the shared renderer
core. The `gpui_wgpu` tests require a native GPU adapter for their GPU-backed
cases. Then run the root crate gates and native Bevy examples documented in
[`docs/maintainers.md`](../../docs/maintainers.md).
