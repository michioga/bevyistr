bevyistr 0.3.0 — Windows x86_64
FrontISTR Pre/Post
Michio Ogawa (michioga)

Extract the entire ZIP to a folder you can write to, then run bevyistr.exe.
Do not run it from inside the ZIP. A GPU and current graphics driver are required.
This build targets Windows x86_64; it is not an ARM64 build.

FrontISTR, MPI, Gmsh and Intel oneAPI are not included. They are not needed to view
existing results. To solve a model, configure your installed FrontISTR in Solve.
VTU/PVTU files include geometry; RES files need their matching mesh.

materials.toml is the editable material library. A file in the current working
directory takes precedence over the file beside the executable. If neither
exists, the embedded catalogue is used. Press Reload materials.toml after editing.

Preferences are saved to %APPDATA%\bevyistr\bevyistr.toml.
BEVYISTR_CONFIG can override this path. Updating this ZIP does not erase settings.
Back up your models and any customized materials.toml before replacing old files.

Manual: https://michioga.github.io/bevyistr/
Source: https://github.com/michioga/bevyistr
Please report the version and source_commit from BUILD-INFO.json with any issue.

LICENSE covers bevyistr; THIRD-PARTY-LICENSES.txt and licenses/ contain dependency
and embedded-asset notices. BUILD-INFO.json records the source, tools, and hashes.
A build with source_dirty=true is a local verification candidate, not a release.

The MSVC C runtime is statically linked in this distribution. DLL-DEPENDENCIES.txt
lists direct PE imports; it does not enumerate GPU drivers loaded at runtime.
This binary is unsigned. SHA256SUMS.txt verifies the ZIP contents against the
provided digest; it is not a code signature.

Known scope limits: this release does not edit every FrontISTR keyword.
Unsupported imported settings block Export/Run to prevent data loss.
The flow tutorial remains under investigation with FrontISTR support.
