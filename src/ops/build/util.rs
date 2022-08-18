use crate::config::{AndroidBuildTarget, AndroidConfig};
use anyhow::format_err;
use cargo::core::{Target, TargetKind, Workspace};
use cargo::util::CargoResult;
use cargo_util::ProcessBuilder;
use std::{
    ffi::OsStr,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use serde::Deserialize;

/// Returns the directory in which all cargo apk artifacts for the current
/// debug/release configuration should be produced.
pub fn get_root_build_directory(workspace: &Workspace, config: &AndroidConfig) -> PathBuf {
    let android_artifacts_dir = workspace
        .target_dir()
        .join("android-artifacts")
        .into_path_unlocked();

    if config.release {
        android_artifacts_dir.join("release")
    } else {
        android_artifacts_dir.join("debug")
    }
}

/// Returns the sub directory within the root build directory for the specified target.
pub fn get_target_directory(root_build_dir: &PathBuf, target: &Target) -> CargoResult<PathBuf> {
    let target_directory = match target.kind() {
        TargetKind::Bin => root_build_dir.join("bin"),
        TargetKind::ExampleBin => root_build_dir.join("examples"),
        _ => unreachable!("Unexpected target kind"),
    };

    let target_directory = target_directory.join(target.name());
    Ok(target_directory)
}

/// Returns path to NDK provided make
pub fn make_path(config: &AndroidConfig) -> PathBuf {
    config.ndk_path.join("prebuild").join(HOST_TAG).join("make")
}

/// Returns the path to the LLVM toolchain provided by the NDK
pub fn llvm_toolchain_root(config: &AndroidConfig) -> PathBuf {
    config
        .ndk_path
        .join("toolchains")
        .join("llvm")
        .join("prebuilt")
        .join(HOST_TAG)
}

// Helper function for looking for a path based on the platform version
// Calls a closure for each attempt and then return the PathBuf for the first file that exists.
// Uses approach that NDK build tools use which is described at:
// https://developer.android.com/ndk/guides/application_mk
// " - The platform version matching APP_PLATFORM.
//   - The next available API level below APP_PLATFORM. For example, android-19 will be used when
//     APP_PLATFORM is android-20, since there were no new native APIs in android-20.
//   - The minimum API level supported by the NDK."
pub fn find_ndk_path<F>(platform: u32, path_builder: F) -> CargoResult<PathBuf>
where
    F: Fn(u32) -> PathBuf,
{
    let mut tmp_platform = platform;

    // Look for the file which matches the specified platform
    // If that doesn't exist, look for a lower version
    while tmp_platform > 1 {
        let path = path_builder(tmp_platform);
        if path.exists() {
            return Ok(path);
        }

        tmp_platform -= 1;
    }

    // If that doesn't exist... Look for a higher one. This would be the minimum API level supported by the NDK
    tmp_platform = platform;
    while tmp_platform < 100 {
        let path = path_builder(tmp_platform);
        if path.exists() {
            return Ok(path);
        }

        tmp_platform += 1;
    }

    Err(format_err!("Unable to find NDK file"))
}

// Returns path to clang executable/script that should be used to build the target
pub fn find_clang(
    config: &AndroidConfig,
    build_target: AndroidBuildTarget,
) -> CargoResult<PathBuf> {
    let bin_folder = llvm_toolchain_root(config).join("bin");
    find_ndk_path(config.min_sdk_version, |platform| {
        bin_folder.join(format!(
            "{}{}-clang{}",
            build_target.ndk_llvm_triple(),
            platform,
            EXECUTABLE_SUFFIX_CMD
        ))
    })
    .map_err(|_| format_err!("Unable to find NDK clang"))
}

// Returns path to clang++ executable/script that should be used to build the target
pub fn find_clang_cpp(
    config: &AndroidConfig,
    build_target: AndroidBuildTarget,
) -> CargoResult<PathBuf> {
    let bin_folder = llvm_toolchain_root(config).join("bin");
    find_ndk_path(config.min_sdk_version, |platform| {
        bin_folder.join(format!(
            "{}{}-clang++{}",
            build_target.ndk_llvm_triple(),
            platform,
            EXECUTABLE_SUFFIX_CMD
        ))
    })
    .map_err(|_| format_err!("Unable to find NDK clang++"))
}

// Returns path to ar.
pub fn find_ar(config: &AndroidConfig, build_target: AndroidBuildTarget) -> CargoResult<PathBuf> {
    // NDK r23 renamed <ndk_llvm_triple>-ar to llvm-ar
    let ar_path = llvm_toolchain_root(config)
        .join("bin")
        .join(format!("llvm-ar{}", EXECUTABLE_SUFFIX_EXE));
    if ar_path.exists() {
        Ok(ar_path)
    } else {
        Err(format_err!(
            "Unable to find ar at `{}`",
            ar_path.to_string_lossy()
        ))
    }
}

// Returns path to readelf
pub fn find_readelf(
    config: &AndroidConfig,
    build_target: AndroidBuildTarget,
) -> CargoResult<PathBuf> {
    // NDK r23 renamed <ndk_llvm_triple>-readelf to llvm-readelf
    let readelf_path = llvm_toolchain_root(config)
        .join("bin")
        .join(format!("llvm-readelf{}", EXECUTABLE_SUFFIX_EXE));
    if readelf_path.exists() {
        Ok(readelf_path)
    } else {
        Err(format_err!(
            "Unable to find readelf at `{}`",
            readelf_path.to_string_lossy()
        ))
    }
}

// Returns dir to libunwind.a for the correct architecture
// e.g. ...llvm/prebuilt/linux-x86_64/lib64/clang/14.0.6/lib/linux/i386
pub fn find_libunwind_dir(
    config: &AndroidConfig,
    build_target: AndroidBuildTarget,
) -> CargoResult<PathBuf> {
    let libunwind_dir = llvm_toolchain_root(config).join("lib64").join("clang");
    let clang_ver = libunwind_dir
        .read_dir()?
        .next()
        .expect("Should be at least one clang version")?
        .file_name();
    let libunwind_dir = libunwind_dir
        .join(clang_ver)
        .join("lib")
        .join("linux")
        .join(build_target.clang_arch());

    if libunwind_dir.join("libunwind.a").exists() {
        Ok(libunwind_dir)
    } else {
        Err(format_err!(
            "Unable to find libunwind.a at `{}`",
            libunwind_dir.to_string_lossy()
        ))
    }
}

pub fn find_package_root_path(
    workspace: &Workspace,
    config: &AndroidConfig,
    package_name: &str,
) -> PathBuf {
    use cargo::{
        core::{compiler, resolver},
        ops,
    };

    let specs = cargo::ops::Packages::Default
        .to_package_id_specs(&workspace)
        .unwrap();
    // assuming all the build targets use the same miniquad version
    // which should be always true
    let first_build_target = config
        .build_targets
        .iter()
        .next()
        .expect("Should be at least one build target");
    let requested_kinds = vec![compiler::CompileKind::Target(
        compiler::CompileTarget::new(first_build_target.rust_triple()).unwrap(),
    )];

    let target_data = compiler::RustcTargetData::new(&workspace, &requested_kinds[..]).unwrap();
    let cli_features = resolver::CliFeatures::new_all(false);
    let ws_resolve = cargo::ops::resolve_ws_with_opts(
        &workspace,
        &target_data,
        &requested_kinds,
        &cli_features,
        &specs,
        resolver::HasDevUnits::No,
        resolver::ForceAllTargets::No,
    )
    .unwrap();

    let miniquad_pkg = ws_resolve
        .pkg_set
        .packages()
        .find(|package| package.name() == package_name).expect("cargo quad can't build a non-miniquad package, but no miniquad is found in the dependencies tree!");

    miniquad_pkg.root().to_path_buf()
}

#[derive(Clone, Debug)]
pub struct JavaFiles {
    /// Optional file with a template to be injected into miniquad's MainActivity
    pub main_activity_injects: Vec<PathBuf>,

    /// Extra Java files to compile alongside the app's MainActivity
    /// Global path, local path
    pub java_files: Vec<(PathBuf, PathBuf)>,

    /// Extra .jar files to use in "javac" invocation
    /// "Compile-time" Java dependency
    pub comptime_jar_files: Vec<(PathBuf, PathBuf)>,

    /// Extra .jar files to use in "dex" invocation
    /// "Runtime-time" Java dependency
    pub runtime_jar_files: Vec<(PathBuf, PathBuf)>,

    /// List of services being appended to "metadata.android.service" with
    /// "enabled: true" value
    pub java_services: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct QuadToml {
    main_activity_inject: Option<String>,
    java_files: Option<Vec<String>>,
    comptime_jar_files: Option<Vec<String>>,
    runtime_jar_files: Option<Vec<String>>,
    java_services: Option<Vec<String>>,
    // a special field being filled while toml parsing
    // do not really belong to a toml and this struct!
    #[serde(skip)]
    package_root: PathBuf,
}

fn read_quad_toml(path: &Path) -> Option<QuadToml> {
    let quad_toml_path = path.join("quad.toml");
    if !quad_toml_path.exists() {
        return None;
    }

    let content = {
        let mut file = File::open(quad_toml_path).unwrap();
        let mut content = String::new();
        file.read_to_string(&mut content).unwrap();
        content
    };
    let mut config: QuadToml = toml::from_str(&content)
        .map_err(anyhow::Error::from)
        .unwrap_or_else(|err| panic!("{:?} toml file malformed, {:?}", path, err));

    config.package_root = path.to_owned();

    Some(config)
}

pub fn collect_java_files(workspace: &Workspace, config: &AndroidConfig) -> JavaFiles {
    use cargo::{
        core::{compiler, resolver},
        ops,
    };

    let specs = cargo::ops::Packages::Default
        .to_package_id_specs(&workspace)
        .unwrap();
    // assuming all the build targets use the same miniquad version
    // which should be always true
    let first_build_target = config
        .build_targets
        .iter()
        .next()
        .expect("Should be at least one build target");
    let requested_kinds = vec![compiler::CompileKind::Target(
        compiler::CompileTarget::new(first_build_target.rust_triple()).unwrap(),
    )];

    let target_data = compiler::RustcTargetData::new(&workspace, &requested_kinds[..]).unwrap();
    let cli_features = resolver::CliFeatures::new_all(false);
    let ws_resolve = cargo::ops::resolve_ws_with_opts(
        &workspace,
        &target_data,
        &requested_kinds,
        &cli_features,
        &specs,
        resolver::HasDevUnits::No,
        resolver::ForceAllTargets::No,
    )
    .unwrap();

    let mut res = JavaFiles {
        main_activity_injects: vec![],
        java_files: vec![],
        comptime_jar_files: vec![],
        runtime_jar_files: vec![],
        java_services: vec![],
    };

    let absolute_path = |root: &PathBuf, path: &str| {
        let mut res = root.clone();
        for path_part in path.split("/") {
            res = res.join(path_part);
        }
        res
    };
    ws_resolve
        .pkg_set
        .packages()
        .filter_map(|package| read_quad_toml(package.root()))
        .for_each(|toml| {
            let root = toml.package_root.clone();
            let to_absolute = |x: &Option<Vec<String>>| {
                x.iter()
                    .flatten()
                    .map(|f| (absolute_path(&root, &f), PathBuf::from(f)))
                    .collect::<Vec<_>>()
            };

            res.main_activity_injects
                .extend(toml.main_activity_inject.map(|f| absolute_path(&root, &f)));
            res.java_files.extend(to_absolute(&toml.java_files));
            res.comptime_jar_files
                .extend(to_absolute(&toml.comptime_jar_files));
            res.runtime_jar_files
                .extend(to_absolute(&toml.runtime_jar_files));
            if let Some(ref java_services) = toml.java_services {
                res.java_services.extend(java_services.iter().cloned());
            }
        });
    res
}

/// Returns a ProcessBuilder which runs the specified command. Uses "cmd" on windows in order to
/// allow execution of batch files.
pub fn script_process(cmd: impl AsRef<OsStr>) -> ProcessBuilder {
    if cfg!(target_os = "windows") {
        let mut pb = ProcessBuilder::new("cmd");
        pb.arg("/C").arg(cmd);
        pb
    } else {
        ProcessBuilder::new(cmd)
    }
}

#[cfg(all(target_os = "windows", target_pointer_width = "64"))]
const HOST_TAG: &str = "windows-x86_64";

#[cfg(all(target_os = "windows", target_pointer_width = "32"))]
const HOST_TAG: &str = "windows";

#[cfg(target_os = "linux")]
const HOST_TAG: &str = "linux-x86_64";

#[cfg(target_os = "macos")]
const HOST_TAG: &str = "darwin-x86_64";

// These are executable suffixes used to simplify building commands.
// On non-windows platforms they are empty.

#[cfg(target_os = "windows")]
const EXECUTABLE_SUFFIX_EXE: &str = ".exe";

#[cfg(not(target_os = "windows"))]
const EXECUTABLE_SUFFIX_EXE: &str = "";

#[cfg(target_os = "windows")]
const EXECUTABLE_SUFFIX_CMD: &str = ".cmd";

#[cfg(not(target_os = "windows"))]
const EXECUTABLE_SUFFIX_CMD: &str = "";

#[cfg(target_os = "windows")]
pub const EXECUTABLE_SUFFIX_BAT: &str = ".bat";

#[cfg(not(target_os = "windows"))]
pub const EXECUTABLE_SUFFIX_BAT: &str = "";
