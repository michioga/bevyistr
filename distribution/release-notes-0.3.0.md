# bevyistr 0.3.0

FrontISTR pre/post-processing with probe comparison, CSV export, eigenvalue metadata and clearer result navigation.

- Export the pinned node/element history to CSV and compare up to four probes.
- Preserve eigenvalue metadata and distinguish mode order from time steps.
- Seek through results with a current/total indicator and consistent keyboard controls.
- Prevent Export/Run when imported settings cannot be preserved.
- Edit eigenvalue inputs and independent RES/VTK output intervals.
- Update dependencies to resolve the known vulnerabilities and unsoundness warnings found during preparation.
- Use LUT-free Reinhard tonemapping for both viewport cameras and remove the vulnerable Zstandard dependency path. Model brightness and colors differ from earlier candidates; result values and contour ranges are unchanged.

## Windows package

Extract the entire Windows x86_64 ZIP and run bevyistr.exe. See README.txt for materials and preferences.
FrontISTR, MPI and Gmsh are not bundled; viewing existing results does not require a solver.
SHA256SUMS.txt verifies the download. BUILD-INFO.json records the source and toolchain.
The executable is unsigned. Checksums are not a code signature.

## Known limits

Not all FrontISTR keywords, VTK encodings or changing-topology series are supported.
Zstandard-compressed textures and SMAA lookup textures are disabled in this build.
The fluid tutorial remains pending a FrontISTR support response.
Five unmaintained-dependency advisories remain tracked; these are distinct from vulnerability and unsoundness findings.
CSV exports the current pinned target, not the entire comparison.

[Manual](https://michioga.github.io/bevyistr/) · [Source](https://github.com/michioga/bevyistr)

Validation scope and remaining platform limits are recorded in the [release checklist](https://github.com/michioga/bevyistr/blob/main/docs/release-0.3.0.md).
