# bevyistr 0.3.0

Release preparation: this text is a draft.

- Export the pinned node/element history to CSV and compare up to four probes.
- Preserve eigenvalue metadata and distinguish mode order from time steps.
- Seek through results with a current/total indicator and consistent keyboard controls.
- Prevent Export/Run when imported settings cannot be preserved.
- Edit eigenvalue inputs and independent RES/VTK output intervals.
- Update dependencies to resolve the known vulnerabilities and unsoundness warnings found during preparation.

## Windows package

Extract the entire Windows x86_64 ZIP and run bevyistr.exe. See README.txt for materials and preferences.
FrontISTR, MPI and Gmsh are not bundled; viewing existing results does not require a solver.
SHA256SUMS.txt verifies the download. BUILD-INFO.json records the source and toolchain.
The executable is unsigned. Checksums are not a code signature.

## Known limits

Not all FrontISTR keywords, VTK encodings or changing-topology series are supported.
The fluid tutorial remains pending a FrontISTR support response.
Five unmaintained-dependency advisories remain tracked; these are distinct from vulnerability and unsoundness findings.
CSV exports the current pinned target, not the entire comparison.

[Manual](https://michioga.github.io/bevyistr/) · [Source](https://github.com/michioga/bevyistr)

Before publishing: confirm final Windows/Linux CI, ZIP smoke tests and all eleven crates.io packages, and replace this preparation note with the verified release date and scope.
