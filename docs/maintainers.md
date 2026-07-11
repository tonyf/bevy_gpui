# Maintainer guide

This guide covers release-level validation, the vendored GPUI fork, examples,
screenshots, compatibility claims, and documentation upkeep.

## Repository boundaries

The root crate contains the Bevy-specific adapter. `vendor/gpui-ce` contains a
pinned upstream GPUI tree plus the smallest host-neutral embedding changes
needed by the adapter.

Keep Bevy-specific types out of the vendored GPUI APIs. The fork should expose
generic seams such as an embedded application lifetime, sendable scene
snapshots, external-surface IDs, and rendering into a caller-owned target.

## Run the release validation matrix

Root crate:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --lib --no-default-features
cargo test --lib
cargo test --examples
cargo test --doc
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps
ruby scripts/check-docs.rb
ruby scripts/check-wgpu-identity.rb
git diff --check
```

Vendored GPUI:

```bash
cargo test --manifest-path vendor/gpui-ce/Cargo.toml -p gpui --lib
cargo test --manifest-path vendor/gpui-ce/Cargo.toml -p gpui_wgpu --lib
```

The `gpui_wgpu` suite includes native GPU work. Run it with access to a real
adapter and record the OS, GPU, and backend. CI is configured to run focused
embedding and external-renderer tests on Linux, macOS, and Windows.

Documentation must use only the current `bevy_gpui` project name and GPUI
terminology. Include retired-branding checks in migration reviews without
preserving the retired names in permanent project documentation.

## Validate examples natively

Each example is a native smoke case. `cargo test --examples` compiles every
example but does not launch its application loop; only embedded unit tests run
under that command.

| Example | Required observation |
|---|---|
| `getting_started` | One Bevy window displays a clickable retained panel |
| `overlay_3d` | UI clicks do not reach scene input or picking; scene clicks still do |
| `text_input` | Keyboard text and IME preedit/commit reach the focused field |
| `multi_window` | Each Bevy window receives only its own root and clicks |
| `render_to_texture` | GPUI renders into a Bevy image and samples a Bevy image without inversion |
| `lifecycle` | Both explicit-context removal and window closure reduce the root count to zero |
| `hdr_overlay` | GPUI remains visible over the floating-point camera target |

Do not replace native checks with static review for window, input, renderer, or
GPU ownership changes.

## Capture screenshots

Store one PNG per stable example in `screenshots/`:

```text
screenshots/overlay_3d.png
screenshots/text_input.png
screenshots/multi_window-primary.png
screenshots/multi_window-secondary.png
screenshots/render_to_texture.png
screenshots/lifecycle-primary.png
screenshots/lifecycle-secondary.png
screenshots/hdr_overlay.png
```

Use a consistent window scale and capture only after the example reaches its
settled state. For interaction screenshots, preserve counter
values that prove UI input did not leak into the scene. Inspect every image at
full resolution before replacing the checked-in artifact.

Cross-platform screenshots must name their platform or live in platform
subdirectories once more than one native platform is checked in.

## Update the vendored GPUI revision

1. Read the exact upstream revision and changed-file inventory from
   [`BEVY_GPUI_PATCH.md`](../vendor/gpui-ce/BEVY_GPUI_PATCH.md).
2. Clone that revision into a disposable directory and compare it with the
   plain vendored directory. `vendor/gpui-ce` is not a nested Git repository.

   ```bash
   (
     upstream=$(mktemp -d)
     trap 'rm -rf "$upstream"' EXIT
     git clone https://github.com/gpui-ce/gpui-ce.git "$upstream"
     git -C "$upstream" checkout 20340e14874a3b55122e5cb2aa0d023874e08b2d
     git diff --no-index --stat -- \
       "$upstream/crates/gpui/src/app.rs" \
       vendor/gpui-ce/crates/gpui/src/app.rs || {
         rc=$?
         [ "$rc" -eq 1 ] || exit "$rc"
       }
     rsync -rcn --delete --itemize-changes \
       --exclude=.git --exclude=target \
       "$upstream/" vendor/gpui-ce/
   )
   ```

   Add a no-index comparison for every file in the patch inventory before the
   subshell exits. The checksum dry run audits the pruned tree as a whole.
3. Fetch and inspect the intended upstream revision outside the root crate's
   implementation changes. Do not overwrite local patches blindly.
4. Reapply each host-neutral change as a small, reviewable patch:

   - `EmbeddedApplication` lifecycle and reentrancy boundaries;
   - `SceneSnapshot: Send + Sync` and batch ordering;
   - external-surface IDs;
   - `SceneRenderer` using a caller-owned encoder and texture view;
   - pointer-hit reporting used by Bevy input ownership;
   - physical key identity.

5. Update [`BEVY_GPUI_PATCH.md`](../vendor/gpui-ce/BEVY_GPUI_PATCH.md) with the
   new exact revision and any changed patch list.
6. Confirm Cargo resolves one WGPU package identity:

   ```bash
   ruby scripts/check-wgpu-identity.rb
   ```

   The script reads Cargo package IDs, including their source, and fails unless
   exactly one `wgpu` identity is present.
7. Run the full vendored and root validation matrices.
8. Launch every native example, with special attention to text, filters,
   external images, and render-device recovery.

## Change the public API

When a public type, method, field, feature, default, or error changes:

1. Update its Rustdoc in `src/`.
2. Update [reference.md](reference.md).
3. Update the relevant tutorial or how-to page.
4. Update [compatibility.md](compatibility.md) if support changes.
5. Add an `[Unreleased]` changelog entry.
6. Add or update a compile-tested example.

`#![warn(missing_docs)]` catches absent Rustdoc but cannot catch stale prose or
incorrect examples.

## Update compatibility claims

Use these labels consistently:

- **Runtime tested:** launched and interacted with on that native platform.
- **Compile/test validated:** CI compiled and ran non-visual tests there.
- **Pending:** implementation exists but the stated evidence has not been run.
- **Unsupported:** no correct implementation is claimed.

Do not promote a platform from compile-tested to runtime-tested based only on
CI. Do not call HDR or alpha visually correct based only on pipeline creation.

## Prepare a release

The crate currently sets `publish = false`. Before the first public release:

1. Decide whether the vendored GPUI crates can be packaged by crates.io or must
   move to published/git dependencies.
2. Add repository and readme metadata suitable for the chosen distribution.
3. Verify the package contents with `cargo package --list` after enabling
   packaging.
4. Confirm the license and upstream attribution for vendored sources.
5. Run the full validation and native example matrix.
6. Update version compatibility, changelog links, and installation snippets.
7. Tag only after the release artifact builds from a clean checkout.

## Documentation completion checklist

- Every public item appears in [reference.md](reference.md).
- Every Cargo feature has a default and effect documented.
- The first tutorial reaches a visible result by step three.
- The top tasks have verification and troubleshooting sections.
- Every docs page is reachable from [docs/README.md](README.md).
- `ruby scripts/check-docs.rb` validates local files, anchors, and README
  discoverability.
- `ruby scripts/check-wgpu-identity.rb` proves the root graph contains one WGPU
  package identity.
- Commands match CI and the live manifest.
- Planned behavior is not presented as implemented.
- Unsupported behavior is explicit.
