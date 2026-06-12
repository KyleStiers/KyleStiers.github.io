# KyleStiers.github.io

This repository now hosts a Rust-powered resume website built with [ratzilla](https://github.com/ratatui/ratzilla). The site renders a terminal-style interface in the browser using WebAssembly.

## Stack

- Rust
- ratzilla
- TOML resume content file
- Trunk (WASM bundling)
- mise (optional task runner/tool bootstrap)
- GitHub Pages (deployment)

## Resume Content Editing

All resume content lives in [resume.toml](resume.toml) using a structured schema (profile, experience, research, skills, education, awards, positions, contact).

The Rust app formats these structured fields into TUI sections automatically, so content edits usually require no code changes.

The app loads this file via `include_str!` at build time, so every change is reflected after rebuild (or automatically during `trunk serve`).

To re-extract content from your source PDF without changing project dependencies:

uvx --from 'markitdown[pdf]' markitdown /Users/gnocb/Documents/CV/KMS_CV_April2024.pdf > /tmp/KMS_CV_April2024.md

## Local Testing Options

Recommended baseline: native Trunk commands. This path has the least moving parts and is the most portable across environments.

### Option 1: Native Trunk (recommended)

1. Install trunk:

   cargo install --locked trunk

2. Add the WASM target:

   rustup target add wasm32-unknown-unknown

3. Serve locally:

   trunk serve

4. Open [http://localhost:8080](http://localhost:8080)

### Option 2: mise task wrapper (optional)

If you already use mise in other projects, this repository includes [mise.toml](mise.toml) with reusable tasks.

This setup intentionally does not pin or auto-install toolchains, to avoid extra friction. It uses your existing local Rust and Trunk installs.

1. Trust the project config once:

   mise trust

2. Start the local dev server:

   mise run --raw serve

3. Optional utility tasks:

   - `mise run check`
   - `mise run build`
   - `mise run --raw serve-no-open`
   - `mise run --raw serve-8081`
   - `mise run stop-serve`
   - `mise run serve-bg` (start in background)
   - `mise run serve-status` (confirm listener)
   - `mise run serve-log` (tail logs)
   - `mise run stop-serve-bg` (stop background server)

If your terminal reports a wrapper-shell "no exit status" for long-running foreground tasks, use the background workflow (`serve-bg`, `serve-log`, `stop-serve-bg`) or run `trunk serve` directly.

### Option 3: Release-like smoke test

1. Build production assets:

   trunk build --release

2. Serve the dist directory locally with any static server:

   python3 -m http.server 4173 --directory dist

3. Open [http://localhost:4173](http://localhost:4173)

This is useful to validate the deploy-like output before pushing.

## Build

trunk build --release

The production output is generated in `dist/`.

## Deployment

Deployment is handled by [GitHub Actions](.github/workflows/deploy.yml).
Pushes to `master` trigger a release build and publish to GitHub Pages.
