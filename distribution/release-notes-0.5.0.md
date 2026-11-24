# bevyistr 0.5.0

## STEP to FrontISTR

- Open a single-solid STEP file in **Open Model** or **Add Part**. Gmsh generates a first-order tetrahedral mesh with CAD face groups for boundary conditions, loads, and contact setup.
- Monitor meshing progress, preview the result, and regenerate with different size and curvature settings. Select contact faces for local refinement before selecting **Use Mesh**. Export the mesh as a FrontISTR `.msh` or use it in a solver project.

## FrontISTR setup and execution

- Re-export preserves supported `!SOLVER` preconditioner, numeric, log, and matrix-dump options, relative result paths, MPI partition controls, and more mesh and section data.
- Reopen supported single-step fixed-increment `ALAGRANGE` contact projects with their friction settings. Review the compatibility messages in **Solve** before exporting imported projects.

## Viewport and Windows

- Upgraded to Bevy 0.20. SolidCraft-style mouse controls replace the separate touchpad presets. Right-drag to orbit and middle-drag to pan.
- Embedded Japanese fonts improve localized paths and error messages, and add icons to common actions. Windows opens the GUI without a console window.
- Extract the Windows x86_64 ZIP before running `bevyistr.exe`. Use `SHA256SUMS.txt` to verify the ZIP and `BUILD-INFO.json` to identify the source and toolchain. The executable is unsigned.

Install FrontISTR and optional MPI separately to run analyses. STEP meshing requires Gmsh on `PATH`. Existing results can be viewed without those tools. [Japanese manual](https://michioga.github.io/bevyistr/) · [Source](https://github.com/michioga/bevyistr)
