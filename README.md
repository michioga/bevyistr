# bevyistr

A Rust/Bevy pre- and post-processor for FrontISTR.

`bevyistr` is read **Bevy Aistar** (ベビーアイスター).

- Rust edition 2024
- Bevy 0.19
- bevy_ui based UI
- FrontISTR oriented (HECMW `.msh` / `.cnt` / `hecmw_ctrl.dat` / `.res` / `.frd` / `.vtu` / `.pvtu`)
- Gmsh MSH v4.1 integration oriented

## Interaction design

The guiding principle is **intuitive operation, rigorous confirmation**.
Geometry is manipulated directly in the 3-D viewport, while engineering
values remain visible and numerically editable. Viewport tools use lightweight
previews, explicit commit/cancel behavior, and a shared lower-right measurement
box for exact values in model units.

## Build & run

```
cargo build --release
./target/release/bevyistr [path/to/mesh.msh | path/to/hecmw_ctrl.dat]
```

Passing a `hecmw_ctrl.dat` loads its mesh together with the boundary
conditions/loads/materials from the `.cnt` file it points to, same as the
GUI's "Open Project" button. Passing a mesh file directly (`.msh`, or
anything the Gmsh v4.1 fallback reads) loads just the mesh, same as "Open
Mesh".
