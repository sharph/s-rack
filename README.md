# s-rack

[![Rust](https://img.shields.io/badge/Rust-%23000000.svg?e&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Rust CI Status](https://github.com/sharph/s-rack/actions/workflows/rust.yml/badge.svg)](https://github.com/sharph/s-rack/actions/workflows/rust.yml)
[![Made with love at the Recurse Center](https://cloud.githubusercontent.com/assets/2883345/11325206/336ea5f4-9150-11e5-9e90-d86ad31993d8.png)](https://www.recurse.com/)

A modular softsynth

![screenshot](screenshot.png)

* [egui](https://github.com/emilk/egui) based UI
* Runs natively or [in the web browser via WASM](https://github.com/emilk/egui)

## Building and running

Like most Rust apps, s-rack uses Cargo to build:

```bash
cargo run
```

or

```bash
cargo build
```

## Developing for web

s-rack uses [Trunk](https://trunkrs.dev/) to manage building for web.

You can `trunk serve` to start a live-reloading environment or `trunk build`
to create a build in `dist/`.
