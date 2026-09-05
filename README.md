# bevyistr

A viewport-first pre/post processor for [FrontISTR](https://gitlab.com/FrontISTR-Commons/FrontISTR), built with Rust, Bevy 0.19, and `bevy_ui`.

`bevyistr` is read **Bevy Aistar** (ベビーアイスター).

The project combines direct 3-D interaction with the numerical precision required by finite-element analysis:

> **Operate intuitively in the viewport; confirm engineering data exactly.**

bevyistr is under active development. It can currently assemble meshes, author and review a useful subset of FrontISTR input, export a complete FrontISTR project, launch FrontISTR directly or through MPI, and inspect common result formats. It does **not** yet expose every FrontISTR keyword.

## Current capabilities

### Model import and assembly

- Open a mesh as a new model or add several meshes as assembly parts.
- Open `hecmw_ctrl.dat` together with its referenced HEC-MW mesh and FrontISTR `.cnt` setup.
- Read HEC-MW `.msh`, Gmsh ASCII MSH 4.1 or newer, Gmsh `.geo`, and basic Abaqus/CalculiX `.inp` meshes.
- Preserve HEC-MW groups and Gmsh physical groups.
- Move or rotate parts with the viewport gizmo, grouped axis controls, or an exact value entered in the lower-right measurement box.
- Reset part poses and inspect clearance, touching, or interference against every other part. Avian is used only for these explicit geometric queries; it is not the FEM or picking engine.
- Navigate with an orbit camera and an axis-aligned view cube.

### SketchUp-inspired selection

- Select Node, Edge, Face, or Element topology directly in the viewport.
- Click or drag to replace, `Ctrl` to add, `Shift` to toggle, and `Alt` or `Ctrl+Shift` to remove.
- Double-click connected boundaries and triple-click connected components.
- Drag left-to-right for fully enclosed box selection or right-to-left for crossing/touching selection.
- Use Single, Coplanar, and Smooth growth modes where they apply.
- Follow continuous feature lines in Edge mode and preview growth results on hover.
- Save selected topology as HEC-MW node or element groups.

The in-application Selection guide shows the active controls and can be collapsed after they become familiar.

### Contact and MPC

- Define `NODE-SURF` and `SURF-SURF` pairs from viewport selections.
- Configure Small sliding, Finite sliding, or Tied behaviour, including exact friction and optional penalty values.
- Detect contact candidates using a model-unit gap and normal-angle tolerance.
- Review candidates with unrelated parts ghosted and an optional exploded-view separation, then accept or reject each pair.
- Create two-node MPC equations for `Ux`, `Uy`, `Uz`, or grouped `XYZ` coupling.
- Detect and review rigid-spider candidates and export accepted constraints as HEC-MW `!EQUATION` entries.
- Review defined equations in the viewport and edit constants or coefficients exactly.

### Boundary conditions and loads

- Author prescribed translations and rotations across all six structural DOFs.
- Author nodal force and moment components.
- Apply pressure to selected faces and gravity with explicit acceleration and direction.
- Interpret rotational values as direct DOFs or about a selected centre node.
- Pick principal load directions in the viewport while keeping exact numeric fields authoritative.
- Preview glyphs before committing with **Apply**.
- Undo or redo analysis-setup changes with `Ctrl+Z`, `Ctrl+Y`, or `Ctrl+Shift+Z`.

### Materials and sections

- Assign project or library materials with an explicit object → material → confirm sequence.
- Edit isotropic Young's modulus, Poisson ratio, and optional density exactly.
- Assign solid, shell, and beam sections, including thickness or area where required.
- Color the viewport by part or material; identical material names use identical colors.
- Undo an assignment, including creation of a new project material, as one operation.

### Solve setup and export

- Load a standalone FrontISTR `.cnt` file into the current mesh.
- Select Static, Nonlinear static, Dynamic, or Eigenvalue analysis.
- Select MUMPS, CG, GMRES, or Direct as the linear solver method; MUMPS is presented first in the UI.
- Enter substeps, maximum iterations, and convergence tolerance exactly.
- Validate references before exporting `hecmw_ctrl.dat`, `<name>.msh`, and `<name>.cnt`.
- Flatten multi-part assemblies while consistently remapping IDs, groups, setup data, contacts, and MPC equations.
- Run `fistr1` directly, or partition the mesh with `hecmw_part1` and then solve through MPI, without blocking the viewport during partitioning/solving. Inspect the stdout/stderr tail and stop local child processes from the Solve page.

**Run FrontISTR** rewrites the current model and setup to the last exported target before launch, so an edited UI state is not solved against stale files. Choose the `fistr1` executable once in the Solve page, put it on `PATH`, or set `FRONTISTR_EXECUTABLE` before starting bevyistr.

- **Direct** runs one `fistr1` process with `HECMW-ENTIRE` input.
- **MPI** follows FrontISTR's [partition-then-solve workflow](https://source-docs.frontistr.com/execution_guide/overview/01_flow.html). It writes `hecmw_part_ctrl.dat` (`TYPE=NODE-BASED, METHOD=PMETIS, DOMAIN=N`) and `hecmw_ctrl.dat` (`part_in`: entire mesh; `part_out` and `fstrMSH`: distributed mesh), runs `hecmw_part1`, checks all N partition files, and only then runs `mpiexec -n N fistr1` (or `mpirun`). Failed or cancelled partitioning never proceeds to the solver. `hecmw_part1` is found beside the selected `fistr1`, then on the effective runtime `PATH`.

Each MPI run uses a fresh `bevyistr_part_*` mesh prefix so old partition files cannot mask missing output. These files remain in the export folder; rerunning Direct or Export restores the entire-mesh control file. **Open Project** uses the original entire mesh (`part_in`) when reopening such a parallel project. The installed partitioner must support PMETIS, and the MPI launcher must match the MPI implementation used to build FrontISTR.

On Windows, an installed Intel oneAPI `setvars.bat` is detected and applied to child processes when Intel MPI is not already configured. Set `FRONTISTR_RUNTIME=inherit` to keep an environment you prepared yourself. Linux inherits the launching environment without invoking this Windows adapter. Process cancellation targets the local run's child processes, not unrelated FrontISTR jobs.

### Results

- Open FrontISTR ASCII `.res.0.N` series, CalculiX ASCII `.frd`, and inline ASCII VTK XML `.vtu`/`.pvtu` results.
- Display scalar or vector-magnitude contours with a color bar.
- Display and scale deformed shapes when displacement data is available.
- Move through result steps manually or animate them with playback and speed controls.

This post-processing UI focuses on convenient inspection. ParaView remains the recommended tool for detailed result analysis.

## Materials workflow

Viewport operations use lightweight previews and explicit commit/cancel behaviour. Exact values remain visible and numerically editable.

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

### Standard material library

The app reads **materials.toml** from its working directory; if absent, it
looks next to **bevyistr.exe**. The Materials panel displays the exact path.
For Cargo runs from the repository root, edit [materials.toml](materials.toml).
For a standalone distribution, ship this file alongside the executable.
The standard file is loaded automatically at startup. Edit the displayed file,
then press **Reload materials.toml** to apply library changes without recompiling
or restarting; reload runs off the UI thread. There is no embedded fallback:
missing files, invalid values, duplicate names, and TOML syntax errors are
reported in the panel without modifying existing model data. Re-select a library
material after reloading; stale drafts cannot be confirmed.

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

## Supported file formats

| Direction | Format | Current behaviour |
|---|---|---|
| Input | HEC-MW `.msh` | Reads mesh topology, groups, materials, sections, contact pairs, and `!EQUATION` data represented by the current model. |
| Input | `hecmw_ctrl.dat` | Resolves and loads the referenced `.msh` and `.cnt` files together. |
| Input | FrontISTR `.cnt` | Reads the setup subset represented by the current UI, including solver settings, BCs, loads, materials, sections, and contact behaviour. |
| Input | Gmsh `.msh` | ASCII MSH 4.x revision 4.1 or newer; physical groups are preserved. Binary MSH is rejected. |
| Input | Gmsh `.geo` | Runs the external `gmsh` command with `-3 -format msh41`, then loads the generated mesh. |
| Input | Abaqus/CalculiX `.inp` | Reads `*NODE`, `*ELEMENT`, `*NSET`, and `*ELSET`; unknown element types remain marked unsupported. |
| Result | FrontISTR `.res.0.N` | Loads one step or detects and loads a numbered ASCII series. |
| Result | CalculiX `.frd` | Reads nodal scalar/vector fields and derives vector magnitude or von Mises values where applicable. |
| Result | VTK XML `.vtu` / `.pvtu` | Reads inline ASCII point data. Binary, base64, appended arrays, and `CellData` are not currently supported. |
| Output | FrontISTR project | Writes `hecmw_ctrl.dat`, HEC-MW `.msh`, and FrontISTR `.cnt`. |

Gmsh conversion currently covers line, triangle, quadrilateral, tetrahedron, hexahedron, and prism families, including the supported quadratic variants. A `.geo` import requires the Gmsh executable to be available on `PATH`.

## HEC-MW element coverage

The following element codes are currently recognized, visualized, and written back to HEC-MW mesh files:

| Family | HEC-MW codes |
|---|---|
| Rod / truss | `111`, `112`, `301` |
| Plane triangle / quadrilateral | `231`, `232`, `241`, `242` |
| Tetrahedron | `341`, `342` |
| Prism | `351`, `352` |
| Hexahedron | `361`, `362` |
| Connector / interface | `511`, `541`, `542` |
| Beam | `611`, `641` |
| Shell | `731`, `732`, `741`, `743`, `761`, `781` |

Unknown HEC-MW element codes are retained as unsupported elements but do not have generated topology for normal visualization or export.

## Typical workflow

1. Use **Open Mesh**, **Add Mesh**, or **Open Project** on Model.
2. Position assembly parts and check clearance where necessary.
3. Create or review groups with the viewport selection tools.
4. Define contacts and MPC equations.
5. Apply BCs, loads, materials, and sections.
6. Configure the analysis and solver on Solve.
7. Export the project, then run FrontISTR directly or through MPI from the Solve page (or run it externally).
8. Open the generated result on Results for contour, deformation, and animation inspection.

## Viewport controls

| Input | Action |
|---|---|
| Middle drag | Orbit |
| `Shift` + middle drag | Pan |
| Mouse wheel | Zoom; over the sidebar it scrolls the panel instead |
| `F` | Focus the current selection |
| View cube | Snap to axis-aligned or corner views |
| `Enter` | Apply the focused exact numeric value |
| `Esc` | Cancel/restore the focused draft; otherwise clear selection |
| `Ctrl+Z` / `Ctrl+Y` | Undo / redo analysis-setup changes |

Tool-specific hints are shown beside the relevant controls. Assembly, contact, BC/load, MPC, and material operations use explicit preview/draft states so an accidental click does not silently become solver input.

## Workspace layout

| Crate | Responsibility |
|---|---|
| `app` | Application entry point and Bevy composition |
| `fem_core` | Mesh, topology, setup, result, contact, and MPC data models |
| `hecmw` | HEC-MW/FrontISTR mesh, control, setup, result, validation, and export I/O |
| `gmsh` | Gmsh CLI bridge and ASCII MSH 4.1+ conversion |
| `interaction`, `picking`, `selection`, `box_select` | Viewport interaction and topology selection |
| `camera` | Orbit/focus camera and navigation cube |
| `visualization` | FEM geometry, highlights, glyphs, material colors, contours, and color bar |
| `ui` | Page workflows and exact numeric editors implemented with `bevy_ui` |

## Current limitations and direction

- Cluster schedulers, remote-job cancellation, structured iteration progress, and solver-error localization in the viewport are not integrated yet. The current runner targets local workstation MPI (`mpiexec` / `mpirun`) and shows text output; exit code 0 alone is not a convergence or model-validity check.
- The UI does not yet expose all FrontISTR analysis types and keywords. Unsupported data may not round-trip through the editable setup model.
- Direct CAD/STEP import and CAD meshing are not implemented; use Gmsh to generate an ASCII MSH 4.1+ mesh.
- Result loading currently attaches fields to the first mesh and is best suited to a single mesh or a flattened exported assembly.
- Merging MPI rank-result files is not implemented yet; use an external post-processor for the complete distributed result.
- VTK XML support is intentionally limited to inline ASCII point data.
- Planned post-processing conveniences include richer result-field selection, hover probes, selected-node history graphs, and interactive clipping. Detailed visualization will continue to rely on ParaView.

The long-term goal is to make the full FrontISTR workflow accessible without returning to a dialog-heavy pre/post interface, while preserving explicit numeric confirmation and valid solver input.

## Build & run

Prerequisites:

- A Rust toolchain supporting edition 2024
- A graphics adapter/backend supported by Bevy/wgpu
- Optional: `gmsh` on `PATH` for `.geo` meshing
- FrontISTR installed separately to solve exported projects (`fistr1` on `PATH`, selected in Solve, or named by `FRONTISTR_EXECUTABLE`)
- For parallel execution: `hecmw_part1` with PMETIS support and an MPI launcher compatible with the installed FrontISTR

Development run:

```text
cargo run --package bevyistr
```

Optional execution overrides are `FRONTISTR_LAUNCH_MODE=mpi`,
`FRONTISTR_MPI_RANKS=<N>` (1–4096), `FRONTISTR_MPI_LAUNCHER=<path-or-name>`,
`FRONTISTR_PARTITIONER=<path-or-name>`, and `FRONTISTR_RUNTIME=inherit`.
They are platform-neutral; normal use can select Direct/MPI and the rank count
from the Solve page instead.

Open an HEC-MW/Gmsh mesh or FrontISTR project directly at startup:

```text
cargo run --package bevyistr -- path/to/model.msh
cargo run --package bevyistr -- path/to/hecmw_ctrl.dat
```

Passing `hecmw_ctrl.dat` at startup loads its mesh together with the BCs,
loads, materials, sections, and solver settings represented by the current
command-line readers. Passing a mesh loads it without a separate `.cnt`, as
with **Open Mesh**. The GUI's **Open Project** path additionally restores the
contact and MPC data represented by the current HEC-MW reader.

Build a release executable:

```text
cargo build --release --package bevyistr
```

Run the workspace checks:

```text
cargo check --workspace
cargo test --workspace
```

## References

- [FrontISTR source documentation](https://source-docs.frontistr.com/)
- [FrontISTR source repository](https://gitlab.com/FrontISTR-Commons/FrontISTR)
- [FrontISTR tutorials](https://gitlab.com/FrontISTR-Commons/FrontISTR/-/tree/master/tutorial?ref_type=heads)
- [Gmsh](https://gmsh.info/)

## License

Copyright (c) 2026 Michio Ogawa (michioga).

Licensed under the [MIT License](LICENSE).
