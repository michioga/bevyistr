bevyistr @VERSION@ — Windows x86_64
FrontISTR Pre/Post
Michio Ogawa (michioga)

Open the MSI, read the MIT license, and select the acceptance checkbox to enable
Install. Launch bevyistr from its desktop shortcut or the Start menu after
setup. The app installs for your Windows account in
%LOCALAPPDATA%\Programs\bevyistr. A GPU with an up-to-date graphics driver
is required.

Install FrontISTR separately to solve a model, then select it in Solve.
FrontISTR, MPI, Gmsh and Intel oneAPI are optional when viewing existing results.
VTU/PVTU files include geometry. RES files need their matching mesh.
To use a working FrontISTR environment from PowerShell, set
$env:FRONTISTR_RUNTIME = 'inherit' and run bevyistr.exe from its installed path
in the same session. In Solve, select the MPI launcher that matches FrontISTR.
For STEP meshing, install Gmsh and put gmsh.exe on PATH. In Model, use Open Model
or Add Part, review the preview, and select Use Mesh.

To customize the material library, copy the installed materials.toml to
%APPDATA%\bevyistr\materials.toml and edit the copy. Press Reload materials.toml
in Materials. A materials.toml in a separate working directory overrides the
user library. The installer updates the default copy in its own directory.

Preferences are saved to %APPDATA%\bevyistr\bevyistr.toml. BEVYISTR_CONFIG can
override the settings path. Updates and uninstall leave those preferences,
the personal material library, and projects outside the install folder in place.
If upgrading from a ZIP, copy any edited materials.toml to the user path above.

Manual: https://michioga.github.io/bevyistr/
Source: https://github.com/michioga/bevyistr
Please include the version and source_commit from BUILD-INFO.json in bug reports.

LICENSE covers bevyistr. THIRD-PARTY-LICENSES.txt and licenses/ contain dependency
and embedded-asset notices. BUILD-INFO.json records the source, tools, and hashes.
source_dirty=true identifies a local verification candidate.

The MSVC C runtime is statically linked. DLL-DEPENDENCIES.txt lists direct PE
imports. SHA256SUMS.txt records the hash of the unsigned MSI.

Export and Run support the FrontISTR settings documented in the manual.
Unsupported imported settings block Export and Run to prevent data loss.
