# bevyistr

<img src="https://raw.githubusercontent.com/michioga/bevyistr/main/assets/bevyistr.png" alt="bevyistr — rainbow finite-element mesh duck" width="160" height="160">

Viewport-first FrontISTR finite-element pre/post processor, built with Rust and Bevy.

Select geometry in the viewport; confirm engineering data numerically. Includes
mesh assembly, contact and boundary-condition setup, material assignment, solver
execution, and RES / VTU / PVTU result visualization with animation and probes.

Version **0.2.0** is [available on crates.io](https://crates.io/crates/bevyistr/0.2.0).

## Installation

```sh
cargo install bevyistr --version 0.2.0 --locked
bevyistr
```

Windows and Linux are intended targets. A Rust toolchain, native build tools,
and a graphics driver compatible with Bevy are required. Windows MSVC builds
require the Windows SDK resource compiler. Linux builds require Bevy's native
window/audio dependencies; see the repository's build instructions.

FrontISTR (and optional MPI / Gmsh) must be installed separately to use those
features. Result viewing does not require the solver. Icons and default material
records are embedded. An external `materials.toml` in the working directory or
beside the executable overrides the bundled defaults; use Reload in Materials.

- [Repository and current limitations](https://github.com/michioga/bevyistr)
- [Japanese manual sources](https://github.com/michioga/bevyistr/tree/main/docs/src)

Under active development; not every FrontISTR keyword or VTK encoding is supported.

MIT License — Copyright (c) 2026 Michio Ogawa (michioga).
