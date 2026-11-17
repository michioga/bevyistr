# bevyistr 0.4.0

This release improves result loading, selection on large meshes, and recovery after a stopped or failed FrontISTR run.

## Results and viewport

- VTU/PVTU series reuse the first frame’s mesh topology. Native ASCII RES parsing allocates less temporary data.
- Changing the display mode reuses the result probe’s spatial index. Large selection groups and Coplanar/Smooth previews reuse mesh data where possible.
- Mouse and touchpad orbit use the screen axes. Choose Mouse, Touchpad (Alt), or Touchpad (Space) in VIEW. Touchpad modes use a modified primary-button drag for orbit, pan, and zoom. The choice is saved in `bevyistr.toml`.

## FrontISTR setup and execution

- Solve can set the `hecmw_part1` and `mpiexec`/`mpirun` paths separately. Auto returns each tool to automatic detection.
- Re-export preserves the default group 1 when `GRPID` is omitted on supported boundary and load cards. Supported `!SOLVER` cards also preserve `iterPREmax`.
- Solve reports input compatibility issues as Blocked before launching the solver. Stopped runs can be started again, and a newer result load takes precedence over an older one.
- The manual lists examples of settings that can be re-exported and settings that block Export or Run.

## Windows package

Extract the Windows x86_64 ZIP and open `bevyistr.exe`. Use `SHA256SUMS.txt` to verify the download and `BUILD-INFO.json` to identify the source commit and Rust toolchain. The executable is unsigned. Install FrontISTR and MPI separately to run analyses. Existing results can be viewed without the solver.

Input authoring covers a subset of FrontISTR. See the manual for supported result encodings, CNT re-export limits, and MPI execution steps. [Manual](https://michioga.github.io/bevyistr/) · [Source](https://github.com/michioga/bevyistr)
