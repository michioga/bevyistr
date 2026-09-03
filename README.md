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

On **Materials**, select a material above the section controls to edit its
isotropic Young's modulus, Poisson ratio, and density directly. **Enter** commits
the focused value; **Esc** or changing the material/page discards uncommitted
text. An empty density means unspecified. Changes apply to every section using
that material and support the setup's **Ctrl+Z / Ctrl+Y** undo/redo.

Material presets use **Pa and kg/m³ (m-kg-s)**. Imported values are retained as
given; no unit conversion is inferred. Check the unit consistency of geometry,
loads, and material constants before export.

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
