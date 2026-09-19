//! Produce Windows resources only for Windows targets. Linux needs no SDK.
use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/bevyistr.png");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RC");
    println!("cargo:rerun-if-env-changed=WINDRES");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let source = image::open("assets/bevyistr.png").expect("Read application icon");
    let frames: Vec<_> = [16, 24, 32, 48, 64, 128, 256]
        .into_iter()
        .map(|size| {
            let rgba = source
                .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
                .into_rgba8();
            image::codecs::ico::IcoFrame::as_png(&rgba, size, size, image::ExtendedColorType::Rgba8)
                .expect("Encode icon size")
        })
        .collect();
    let ico = out.join("bevyistr.ico");
    image::codecs::ico::IcoEncoder::new(fs::File::create(&ico).unwrap())
        .encode_images(&frames)
        .expect("Write multi-resolution icon");
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let numeric = ["MAJOR", "MINOR", "PATCH"]
        .map(|p| env::var(format!("CARGO_PKG_VERSION_{p}")).unwrap())
        .join(",");
    let rc = out.join("bevyistr.rc");
    fs::write(
        &rc,
        format!(
            r#"1 ICON "{}"
1 VERSIONINFO
FILEVERSION {numeric},0
PRODUCTVERSION {numeric},0
FILEOS 0x40004
FILETYPE 1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "bevyistr - FrontISTR Pre/Post\0"
      VALUE "FileVersion", "{version}\0"
      VALUE "ProductName", "bevyistr\0"
      VALUE "ProductVersion", "{version}\0"
      VALUE "LegalCopyright", "Copyright (c) 2026 Michio Ogawa (michioga)\0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
"#,
            ico.display().to_string().replace('\\', "/")
        ),
    )
    .unwrap();
    let msvc = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let object = out.join(if msvc { "bevyistr.res" } else { "bevyistr.o" });
    let mut compiler = if msvc {
        let host_arch = env::var("HOST")
            .unwrap()
            .split('-')
            .next()
            .unwrap()
            .to_owned();
        let sdk_rc = find_msvc_tools::find_windows_sdk(&host_arch)
            .and_then(|sdk| sdk.path().map(|p| p.join("rc.exe")).find(|p| p.is_file()));
        Command::new(
            env::var_os("RC")
                .map(PathBuf::from)
                .or(sdk_rc)
                .unwrap_or_else(|| "rc.exe".into()),
        )
    } else {
        Command::new(env::var_os("WINDRES").unwrap_or_else(|| "windres".into()))
    };
    if msvc {
        compiler.arg("/nologo").arg("/fo").arg(&object).arg(&rc);
    } else {
        compiler
            .arg("-i")
            .arg(&rc)
            .arg("-o")
            .arg(&object)
            .args(["-O", "coff"]);
    }
    let status = compiler.status().expect("Run Windows resource compiler (Windows SDK rc.exe or MinGW windres; override with RC/WINDRES)");
    assert!(status.success(), "Windows resource compilation failed");
    println!("cargo:rustc-link-arg-bin=bevyistr={}", object.display());
}
