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

On **Materials**, follow **1 Select object → 2 Select material → 3 Confirm**:

1. Click an object in the viewport, or choose a part / named element group in
   the first panel. A cyan bounding outline identifies the target without
   hiding its material color. Hover uses a yellow outline. Ordinary
   Node/Edge/Face/Element growth controls are not shown in this workflow.
2. Choose a material already in the model, or one from the external TOML
   library. For a library entry, explicitly choose **m / kg / N / s**
   (Pa, kg/m³) or **mm / t / N / s** (MPa, t/mm³).
3. **Confirm** adds/reuses the material and assigns it in one undoable operation.
   Choosing a library entry alone changes nothing in the analysis setup.
   Changing the target or pressing **Esc** cancels the pending choice.

Whole-part assignment preserves existing section thickness/area and adds a
fallback section when needed. Named groups can override the fallback. New
section options appear only when necessary; thickness/area is hidden for
solids. **Ctrl+Z / Ctrl+Y** undo/redo the complete assignment, including a new
library material. If a different project material already has the same name,
the new entry receives a unique suffix shown on the confirmation button.

After selecting a project material, its isotropic Young's modulus, Poisson
ratio, and density can be edited directly. **Enter** commits the focused
property; **Esc** or changing material/page discards uncommitted text. An empty
density means unspecified. Property edits affect every section using that
project material, independently of assignment confirmation.

### External material library

The app reads **materials.toml** from its working directory; if absent, it
looks next to **bevyistr.exe**. The Materials panel displays the exact path.
For Cargo runs from the repository root, edit [materials.toml](materials.toml).
For a standalone distribution, ship this file alongside the executable.
**Open TOML...** selects another file for the current session; **Reload** reads
the current file again. No recompilation or restart is needed. The file picker
and reload run off the UI thread. There is no embedded fallback: missing files,
invalid values, duplicate names, and TOML syntax errors are reported in the
panel without modifying existing model data. Re-select a library material after
reloading; stale drafts cannot be confirmed.

Files are UTF-8 (a BOM is accepted), up to 1 MiB. Add entries as follows:

```toml
schema_version = 1

[[materials]]
name = "MY_STEEL"
label = "My steel"
young_pa = 210e9
poisson = 0.3
density_kg_m3 = 7850
source = "Replace with your verified source"
note = "Specify the applicable grade, temperature and conditions"
```

Names are unique HECMW identifiers (letters, digits, underscores).
The storage units are always **Pa** and **kg/m³**, regardless of display/model
units. Density may be omitted; label, source, source_url, and note are optional.
Only the copied library values are converted. Geometry, loads, and imported
materials are never rescaled, and project edits never rewrite the TOML file.

The supplied file starts with generic steel, aluminum 6082, stainless 301L,
and Ti-6Al-4V, with source URLs and applicability notes. These are basic
isotropic elastic reference constants, not certified design allowables or
complete plastic/thermal models. The titanium modulus is explicitly the
midpoint of the source range. Verify constants and unit consistency for the
actual analysis.

The shared **VIEW** toolbar switches between **Color: Part** and
**Color: Material** (default). The same material name always uses the same
base color across parts; swatches appear in the material selector. Unassigned
elements are gray; missing/ambiguous material references or competing group
materials are pink. Names remain the authoritative identity, since colors can
be similar. Selection/hover highlights and active result contours take visual
priority; clearing a contour restores the current material colors.

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
