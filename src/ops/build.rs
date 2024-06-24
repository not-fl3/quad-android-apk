// some really useful links:
// https://stackoverflow.com/questions/59504840/create-jni-ndk-apk-only-command-line-without-gradle-ant-or-cmake/59533703#59533703
//
mod compile;
mod preprocessor;
mod targets;
pub mod tempfile;
mod util;

use self::compile::SharedLibraries;
use crate::config::{AndroidConfig, AndroidTargetConfig};
use anyhow::format_err;
use cargo::{
    core::{compiler, resolver, Target, TargetKind, Workspace},
    ops,
    util::CargoResult,
};
use cargo_util::ProcessBuilder;
use clap::ArgMatches;

use std::{
    collections::BTreeMap,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    {env, fs},
};

#[derive(Debug)]
pub struct BuildResult {
    /// Mapping from target kind and target name to the built APK
    pub target_to_apk_map: BTreeMap<(TargetKind, String), PathBuf>,
}

pub fn build(
    workspace: &Workspace,
    config: &AndroidConfig,
    options: &ArgMatches,
) -> CargoResult<BuildResult> {
    let root_source_path = workspace.root();
    let root_build_dir = util::get_root_build_directory(workspace, config);
    let miniquad_root_path = util::find_package_root_path(workspace, config, "miniquad");
    let java_files = util::collect_java_files(workspace, config);
    let shared_libraries = compile::build_shared_libraries(
        workspace,
        config,
        options,
        &root_build_dir,
        &miniquad_root_path,
    )?;
    let sign = !options.get_flag("nosign");

    build_apks(
        config,
        root_source_path,
        &root_build_dir,
        shared_libraries,
        java_files,
        sign,
        &miniquad_root_path,
    )
}

fn build_apks(
    config: &AndroidConfig,
    root_source_path: &Path,
    root_build_dir: &PathBuf,
    shared_libraries: SharedLibraries,
    java_files: util::JavaFiles,
    sign: bool,
    miniquad_root_path: &PathBuf,
) -> CargoResult<BuildResult> {
    let main_activity_path = miniquad_root_path.join("java").join("MainActivity.java");
    let quad_native_path = miniquad_root_path.join("java").join("QuadNative.java");

    // Create directory to hold final APKs which are signed using the debug key
    let final_apk_dir = root_build_dir.join("apk");
    fs::create_dir_all(&final_apk_dir)?;

    // Paths of created APKs
    let mut target_to_apk_map = BTreeMap::new();

    // Build an APK for each cargo target
    for (target, shared_libraries) in shared_libraries.shared_libraries.iter_all() {
        let target_directory = util::get_target_directory(root_build_dir, target)?;

        fs::create_dir_all(&target_directory)?;

        // Determine Target Configuration
        let target_config = config.resolve((target.kind().to_owned(), target.name().to_owned()))?;

        //
        // Run commands to produce APK
        //
        build_manifest(
            &target_directory,
            &config,
            &target_config,
            &target,
            &java_files,
        )?;

        let build_tools_path = config
            .sdk_path
            .join("build-tools")
            .join(&config.build_tools_version);
        let aapt_path = build_tools_path.join("aapt");
        let d8_path = build_tools_path.join("d8");
        let zipalign_path = build_tools_path.join("zipalign");

        // Create unaligned APK which includes resources and assets
        let unaligned_apk_name = format!("{}_unaligned.apk", target.name());
        let unaligned_apk_path = target_directory.join(&unaligned_apk_name);
        if unaligned_apk_path.exists() {
            std::fs::remove_file(unaligned_apk_path)
                .map_err(|e| format_err!("Unable to delete APK file. {}", e))?;
        }

        let obj_dir = target_directory.join("build").join("obj");
        fs::create_dir_all(&obj_dir)?;

        let gen_dir = target_directory.join("build").join("gen");
        fs::create_dir_all(&gen_dir)?;

        let package_name = target_config.package_name.replace("-", "_");
        let library_name = target_config
            .package_name
            .split(".")
            .last()
            .unwrap()
            .clone();

        let mut r_java_path = gen_dir.clone();
        for file_part in package_name.split('.') {
            r_java_path = r_java_path.join(file_part);
        }

        let mut java_dir = target_directory.clone();
        for file_part in package_name.split('.') {
            java_dir = java_dir.join(file_part);
        }
        fs::create_dir_all(&java_dir)?;

        let target_activity_path = java_dir.join("MainActivity.java");

        let java_src = fs::read_to_string(&main_activity_path)
            .expect("Something went wrong reading miniquad's MainActivity.java file");

        let java_src = preprocessor::preprocess_main_activity(
            &java_src,
            &package_name,
            &library_name,
            &java_files.main_activity_injects,
        );

        fs::write(&target_activity_path, java_src)?;

        let target_quad_native_path = target_directory.join("quad_native").join("QuadNative.java");
        fs::create_dir_all(target_quad_native_path.parent().unwrap())?;
        fs::copy(&quad_native_path, &target_quad_native_path)?;

        for (global_path, local_path) in &java_files.java_files {
            let java_src = fs::read_to_string(global_path)
                .expect("Something went wrong reading miniquad's MainActivity.java file");

            let java_src = java_src.replace("TARGET_PACKAGE_NAME", &package_name);
            let java_src = java_src.replace("LIBRARY_NAME", &library_name);

            let local_path = local_path.strip_prefix("java/")?;

            let target_path = target_directory.join(&local_path);
            let dir_path = local_path.parent().unwrap();
            fs::create_dir_all(target_directory.join(dir_path))?;

            fs::write(&target_path, java_src)?;
        }

        let res_dir = target_directory.join("res").join("layout");
        fs::create_dir_all(&res_dir)?;
        let res_file = res_dir.join("main.xml");
        let mut res_file = File::create(&res_file)?;
        writeln!(
            res_file,
            "{}",
            r##"<?xml version="1.0" encoding="utf-8"?>
        <LinearLayout xmlns:android="http://schemas.android.com/apk/res/android"
            android:orientation="vertical"
            android:layout_width="fill_parent"
            android:layout_height="fill_parent"
            >
        </LinearLayout>
        "##
        );

        let mut aapt_package_cmd = ProcessBuilder::new(&aapt_path);
        aapt_package_cmd
            .arg("package")
            .arg("-F")
            .arg(&unaligned_apk_name)
            .arg("-m")
            .arg("-J")
            .arg("build/gen")
            .arg("-M")
            .arg("AndroidManifest.xml")
            .arg("-S")
            .arg("res")
            .arg("-I")
            .arg(&config.android_jar_path);

        if let Some(res_path) = target_config.res_path {
            aapt_package_cmd.arg("-S").arg(res_path);
        }

        // Link assets
        if let Some(assets_path) = &target_config.assets_path {
            aapt_package_cmd.arg("-A").arg(assets_path);
        }

        aapt_package_cmd.cwd(&target_directory).exec()?;

        let mut classpath = config.android_jar_path.to_str().unwrap().to_string();
        for (comptime_jar, _) in &java_files.comptime_jar_files {
            classpath.push_str(":");
            classpath.push_str(comptime_jar.to_str().unwrap());
        }

        let javac_filename = if cfg!(target_os = "windows") {
            "javac.exe"
        } else {
            "javac"
        };
        let javac_path = find_java_executable(javac_filename)?;

        let rt_jar_path = find_rt_jar()?;

        let mut java_cmd = ProcessBuilder::new(javac_path);
        java_cmd
            .arg("-source")
            .arg("1.7")
            .arg("-target")
            .arg("1.7")
            .arg("-Xlint:deprecation")
            .arg("-bootclasspath")
            .arg(rt_jar_path)
            .arg("-classpath")
            .arg(&classpath)
            .arg("-d")
            .arg("build/obj");
        java_cmd.arg("quad_native/QuadNative.java");
        for (_, java_file) in &java_files.java_files {
            let java_file = java_file.strip_prefix("java/")?;
            java_cmd.arg(&java_file);
        }
        java_cmd
            .arg(r_java_path.join("R.java"))
            .arg(target_activity_path);

        java_cmd.cwd(&target_directory).exec()?;

        let mut d8_cmd = ProcessBuilder::new(&d8_path);
        for class_file in glob::glob(target_directory.join("**/*.class").to_str().unwrap()).unwrap()
        {
            let file = class_file.unwrap();
            d8_cmd.arg(file.to_str().unwrap());
        }
        for (runtime_jar, _) in &java_files.runtime_jar_files {
            d8_cmd.arg(&runtime_jar);
        }
        // otherwise "Type `java.lang.System` was not found" error
        d8_cmd.arg("--no-desugaring");
        d8_cmd.arg("--min-api").arg("26");

        d8_cmd.cwd(&target_directory).exec()?;

        ProcessBuilder::new(&aapt_path)
            .arg("add")
            .arg(&unaligned_apk_name)
            .arg("classes.dex")
            .cwd(&target_directory)
            .exec()?;

        // Add shared libraries to the APK
        for shared_library in shared_libraries {
            // Copy the shared library to the appropriate location in the target directory and with the appropriate name
            // Note: that the type of slash used matters. This path is passed to aapt and the shared library
            // will not load if backslashes are used.
            let so_path = format!(
                "lib/{}/{}",
                &shared_library.abi.android_abi(),
                shared_library.filename
            );

            let target_shared_object_path = target_directory.join(&so_path);
            fs::create_dir_all(target_shared_object_path.parent().unwrap())?;
            fs::copy(&shared_library.path, target_shared_object_path)?;

            // Add to the APK
            ProcessBuilder::new(&aapt_path)
                .arg("add")
                .arg(&unaligned_apk_name)
                .arg(so_path)
                .cwd(&target_directory)
                .exec()?;
        }

        // Determine the directory in which to place the aligned and signed APK
        let target_apk_directory = match target.kind() {
            TargetKind::Bin => final_apk_dir.clone(),
            TargetKind::ExampleBin => final_apk_dir.join("examples"),
            _ => unreachable!("Unexpected target kind"),
        };
        fs::create_dir_all(&target_apk_directory)?;

        // Align apk
        let final_apk_path = target_apk_directory.join(format!("{}.apk", target.name()));
        ProcessBuilder::new(&zipalign_path)
            .arg("-f")
            .arg("-v")
            .arg("4")
            .arg(&unaligned_apk_name)
            .arg(&final_apk_path)
            .cwd(&target_directory)
            .exec()?;

        // Find or generate a debug keystore for signing the APK
        // We use the same debug keystore as used by the Android SDK. If it does not exist,
        // then we create it using keytool which is part of the JRE/JDK
        let android_directory = dirs::home_dir()
            .ok_or_else(|| format_err!("Unable to determine home directory"))?
            .join(".android");
        fs::create_dir_all(&android_directory)?;
        let keystore_path = android_directory.join("debug.keystore");
        if !keystore_path.exists() {
            // Generate key
            let keytool_filename = if cfg!(target_os = "windows") {
                "keytool.exe"
            } else {
                "keytool"
            };

            let keytool_path = find_java_executable(keytool_filename)?;
            ProcessBuilder::new(keytool_path)
                .arg("-genkey")
                .arg("-v")
                .arg("-keystore")
                .arg(&keystore_path)
                .arg("-storepass")
                .arg("android")
                .arg("-alias")
                .arg("androidebugkey")
                .arg("-keypass")
                .arg("android")
                .arg("-dname")
                .arg("CN=Android Debug,O=Android,C=US")
                .arg("-keyalg")
                .arg("RSA")
                .arg("-keysize")
                .arg("2048")
                .arg("-validity")
                .arg("10000")
                .cwd(root_build_dir)
                .exec()?;
        }

        if sign {
            // Sign the APK with the development certificate
            util::script_process(
                build_tools_path.join(format!("apksigner{}", util::EXECUTABLE_SUFFIX_BAT)),
            )
            .arg("sign")
            .arg("--ks")
            .arg(keystore_path)
            .arg("--ks-pass")
            .arg("pass:android")
            .arg(&final_apk_path)
            .cwd(&target_directory)
            .exec()?;
        }
        target_to_apk_map.insert(
            (target.kind().to_owned(), target.name().to_owned()),
            final_apk_path,
        );
    }

    Ok(BuildResult { target_to_apk_map })
}

/// Find an executable that is part of the Java SDK
fn find_java_executable(name: &str) -> CargoResult<PathBuf> {
    // Look in PATH
    env::var_os("PATH")
        .and_then(|paths| {
            env::split_paths(&paths)
                .filter_map(|path| {
                    let filepath = path.join(name);
                    if fs::metadata(&filepath).is_ok() {
                        Some(filepath)
                    } else {
                        None
                    }
                })
                .next()
        })
        .or_else(||
            // Look in JAVA_HOME
            env::var_os("JAVA_HOME").and_then(|java_home| {
                let filepath = PathBuf::from(java_home).join("bin").join(name);
                if filepath.exists() {
                    Some(filepath)
                } else {
                    None
                }
            }))
        .ok_or_else(|| {
            format_err!(
                "Unable to find executable: '{}'. Configure PATH or JAVA_HOME with the path to the JRE or JDK.",
                name
            )
        })
}

fn find_rt_jar() -> CargoResult<String> {
    let java_filename = if cfg!(target_os = "windows") {
        "java.exe"
    } else {
        "java"
    };
    let java_path = find_java_executable(java_filename)?;

    let mut res = None;
    let mut cmd = ProcessBuilder::new(&java_path)
        .arg("-verbose")
        .exec_with_streaming(
            &mut |stdout: &str| {
                if stdout.contains("Opened") && stdout.contains("rt.jar") {
                    res = Some(stdout[8..stdout.len() - 1].to_string());
                }

                Ok(())
            },
            &mut |_| Ok(()),
            false,
        );

    if res.is_none() {
        panic!("rt.jar cant be found, probably JRE is not installed");
    }
    Ok(res.unwrap())
}
fn build_manifest(
    path: &Path,
    config: &AndroidConfig,
    target_config: &AndroidTargetConfig,
    target: &Target,
    java_files: &util::JavaFiles,
) -> CargoResult<()> {
    let file = path.join("AndroidManifest.xml");
    let mut file = File::create(&file)?;

    // Building application attributes
    let application_attrs = format!(
        r#"
            android:hasCode="true" android:label="{0}"{1}{2}{3}"#,
        target_config.package_label,
        target_config
            .package_icon
            .as_ref()
            .map_or(String::new(), |a| format!(
                r#"
            android:icon="{}""#,
                a
            )),
        if target_config.fullscreen {
            r#"
            android:theme="@android:style/Theme.DeviceDefault.NoActionBar.Fullscreen""#
        } else {
            ""
        },
        target_config
            .application_attributes
            .as_ref()
            .map_or(String::new(), |a| a.replace("\n", "\n            "))
    );

    // Build activity attributes
    let activity_attrs = format!(
        r#"
                android:name=".MainActivity"
                android:label="{0}"
                android:configChanges="orientation|keyboardHidden|screenSize" {1}"#,
        target_config.package_label,
        target_config
            .activity_attributes
            .as_ref()
            .map_or(String::new(), |a| a.replace("\n", "\n                "))
    );

    let uses_features = target_config
        .features
        .iter()
        .map(|f| {
            format!(
                "\n\t<uses-feature android:name=\"{}\" android:required=\"{}\" {}/>",
                f.name,
                f.required,
                f.version
                    .as_ref()
                    .map_or(String::new(), |v| format!(r#"android:version="{}""#, v))
            )
        })
        .collect::<Vec<String>>()
        .join(", ");

    let uses_permissions = target_config
        .permissions
        .iter()
        .map(|f| {
            format!(
                "\n\t<uses-permission android:name=\"{}\" {max_sdk_version}/>",
                f.name,
                max_sdk_version = f.max_sdk_version.map_or(String::new(), |v| format!(
                    r#"android:maxSdkVersion="{}""#,
                    v
                ))
            )
        })
        .collect::<Vec<String>>()
        .join(", ");

    // <service android:name="" android:enabled="true"></service>

    let services = java_files
        .java_services
        .iter()
        .map(|service| {
            format!(
                "\n\t<service android:name=\"{}\" android:enabled=\"{}\"></service>",
                service, true
            )
        })
        .collect::<Vec<String>>()
        .join(", ");

    // Write final AndroidManifest
    writeln!(
        file,
        r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
        package="{package}"
        android:versionCode="{version_code}"
        android:versionName="{version_name}">
    <uses-sdk android:targetSdkVersion="{targetSdkVersion}" android:minSdkVersion="{minSdkVersion}" />
    <uses-feature android:glEsVersion="{glEsVersion}" android:required="true"></uses-feature>{uses_features}{uses_permissions}
    <application {application_attrs} >
        {services}
        <activity {activity_attrs} >
            <meta-data android:name="android.app.lib_name" android:value="{target_name}" />
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>"#,
        package = target_config.package_name.replace("-", "_"),
        version_code = target_config.version_code,
        version_name = target_config.version_name,
        targetSdkVersion = config.target_sdk_version,
        minSdkVersion = config.min_sdk_version,
        glEsVersion = format!(
            "0x{:04}{:04}",
            target_config.opengles_version_major, target_config.opengles_version_minor
        ),
        uses_features = uses_features,
        uses_permissions = uses_permissions,
        application_attrs = application_attrs,
        activity_attrs = activity_attrs,
        target_name = target.name(),
        services = services
    )?;

    Ok(())
}
