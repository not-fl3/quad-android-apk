use crate::config::AndroidConfig;
use crate::ops::install;
use anyhow::format_err;
use cargo::core::{TargetKind, Workspace};
use cargo::util::CargoResult;
use cargo_util::ProcessBuilder;
use clap::ArgMatches;

pub fn run(workspace: &Workspace, config: &AndroidConfig, options: &ArgMatches) -> CargoResult<()> {
    let build_result = install::install(workspace, config, options)?;

    // Determine the target that should be executed
    let requested_target = if options.get_flag("example") && options.get_flag("bin") {
        return Err(format_err!(
            "Specifying both example and bin targets is not supported"
        ));
    } else if let Some(bin) = options.get_one::<String>("bin") {
        (TargetKind::Bin, bin.clone())
    } else if let Some(example) = options.get_one::<String>("example") {
        (TargetKind::ExampleBin, example.clone())
    } else {
        match build_result.target_to_apk_map.len() {
            1 => build_result
                .target_to_apk_map
                .keys()
                .next()
                .unwrap()
                .to_owned(),
            0 => return Err(format_err!("No APKs to execute.")),
            _ => {
                return Err(format_err!(
                "Multiple APKs built. Specify which APK to execute using '--bin' or '--example'."
            ))
            }
        }
    };

    // Determine package name
    let package_name = config.resolve(requested_target)?.package_name;

    //
    // Start the APK using adb
    //
    let adb = config.sdk_path.join("platform-tools/adb");

    // Found it by doing this :
    //     adb shell "cmd package resolve-activity --brief com.author.myproject | tail -n 1"
    let activity_path = format!("{}/.MainActivity", package_name.replace("-", "_"),);

    drop(writeln!(workspace.gctx().shell().err(), "Running apk"));
    ProcessBuilder::new(&adb)
        .arg("shell")
        .arg("am")
        .arg("start")
        .arg("-a")
        .arg("android.intent.action.MAIN")
        .arg("-n")
        .arg(&activity_path)
        .exec()?;

    Ok(())
}
