#![allow(warnings)]

use anyhow::format_err;
use cargo::core::Workspace;
use cargo::util::{
    command_prelude::{opt, ArgMatchesExt, CommandExt},
    GlobalContext,
};
use cargo_util::ProcessBuilder;
use clap::{Arg, ArgAction, ArgMatches, Command};

mod config;
mod ops;

fn main() {
    let mut cargo_gctx = GlobalContext::default().unwrap();

    let args = match cli().try_get_matches() {
        Ok(args) => args,
        Err(err) => cargo::exit_with_error(err.into(), &mut *cargo_gctx.shell()),
    };

    let args = match args.subcommand() {
        Some(("quad-apk", subcommand_matches)) => subcommand_matches,
        _ => &args,
    };

    let (command, subcommand_args) = match args.subcommand() {
        Some((command, subcommand_args)) => (command, subcommand_args),
        _ => {
            drop(cli().print_help());
            return;
        }
    };

    let arg_target_dir = &subcommand_args.value_of_path("target-dir", &cargo_gctx);

    cargo_gctx
        .configure(
            args.get_count("verbose") as u32,
            args.get_flag("quiet"),
            args.get_one::<String>("color").map(|s| s.as_str()),
            args.get_flag("frozen"),
            args.get_flag("locked"),
            args.get_flag("offline"),
            arg_target_dir,
            &args
                .get_many::<String>("unstable-features")
                .unwrap_or_default()
                .cloned()
                .collect::<Vec<String>>(),
            &[],
        )
        .unwrap();

    let err = match command {
        "build" => execute_build(&subcommand_args, &cargo_gctx),
        "install" => execute_install(&subcommand_args, &cargo_gctx),
        "run" => execute_run(&subcommand_args, &cargo_gctx),
        "logcat" => execute_logcat(&subcommand_args, &cargo_gctx),
        _ => cargo::exit_with_error(
            format_err!(
                "Expected `build`, `install`, `run`, or `logcat`. Got {}",
                command
            )
            .into(),
            &mut *cargo_gctx.shell(),
        ),
    };

    match err {
        Ok(_) => (),
        Err(err) => cargo::exit_with_error(err, &mut *cargo_gctx.shell()),
    }
}

fn cli() -> Command {
    Command::new("cargo-apk")
        .arg(
            Arg::new("verbose")
                .long("verbose")
                .short('v')
                .help("Use verbose output (-vv very verbose/build.rs output)")
                .global(true)
                .action(ArgAction::Count),
        )
        .arg(
            Arg::new("quiet")
                .long("quiet")
                .short('q')
                .help("No output printed to stdout")
                .action(ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            opt("color", "Coloring: auto, always, never")
                .value_name("WHEN")
                .global(true),
        )
        .arg(
            Arg::new("frozen")
                .long("frozen")
                .help("Require Cargo.lock and cache are up to date")
                .action(ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("locked")
                .long("locked")
                .help("Require Cargo.lock is up to date")
                .action(ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("offline")
                .long("offline")
                .help("Run without accessing the network")
                .action(ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("unstable-features")
                .help("Unstable (nightly-only) flags to Cargo, see 'cargo -Z help' for details")
                .short('Z')
                .value_name("FLAG")
                .action(ArgAction::Append)
                .number_of_values(1)
                .global(true),
        )
        .arg(
            Arg::new("nosign")
                .long("nosign")
                .help("Skip \"apksigner\" build step to produced unsigned APK.")
                .action(ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("nostrip")
                .long("nostrip")
                .help("Skip \"striop\" build step, to keep debug symbols even in release builds.")
                .action(ArgAction::SetTrue)
                .global(true),
        )
        .subcommands(vec![
            cli_apk(),
            cli_build(),
            cli_install(),
            cli_run(),
            cli_logcat(),
        ])
}

fn cli_apk() -> Command {
    Command::new("quad-apk")
        .about("dummy subcommand to allow for calling cargo apk instead of cargo-apk")
        .subcommands(vec![cli_build(), cli_install(), cli_run(), cli_logcat()])
}

fn cli_build() -> Command {
    Command::new("build")
        .alias("b")
        .about("Compile a local package and all of its dependencies")
        .arg_package_spec(
            "Package to build (see `cargo help pkgid`)",
            "Build all packages in the workspace",
            "Exclude packages from the build",
        )
        .arg_jobs()
        .arg_targets_all(
            "Build only this package's library",
            "Build only the specified binary",
            "Build all binaries",
            "Build only the specified example",
            "Build all examples",
            "Build only the specified test target",
            "Build all tests",
            "Build only the specified bench target",
            "Build all benches",
            "Build all targets",
        )
        .arg_release("Build artifacts in release mode, with optimizations")
        .arg_features()
        .arg_target_triple("Build for the target triple")
        .arg_target_dir()
        .arg(opt("out-dir", "Copy final artifacts to this directory").value_name("PATH"))
        .arg_profile("Build artifacts with the specified profile")
        .arg_manifest_path()
        .arg_message_format()
        .arg_build_plan()
        .after_help(
            "\
All packages in the workspace are built if the `--all` flag is supplied. The
`--all` flag is automatically assumed for a virtual manifest.
Note that `--exclude` has to be specified in conjunction with the `--all` flag.

Compilation can be configured via the use of profiles which are configured in
the manifest. The default profile for this command is `dev`, but passing
the --release flag will use the `release` profile instead.
",
        )
}

fn cli_install() -> Command {
    Command::new("install")
        .about("Install a Rust binary")
        .arg(
            Arg::new("crate")
                .value_parser(clap::builder::NonEmptyStringValueParser::new())
                .action(ArgAction::Append),
        )
        .arg(
            opt("version", "Specify a version to install from crates.io")
                .alias("vers")
                .value_name("VERSION"),
        )
        .arg(opt("git", "Git URL to install the specified crate from").value_name("URL"))
        .arg(opt("branch", "Branch to use when installing from git").value_name("BRANCH"))
        .arg(opt("tag", "Tag to use when installing from git").value_name("TAG"))
        .arg(opt("rev", "Specific commit to use when installing from git").value_name("SHA"))
        .arg(opt("path", "Filesystem path to local crate to install").value_name("PATH"))
        .arg(opt(
            "list",
            "list all installed packages and their versions",
        ))
        .arg_jobs()
        .arg(opt("force", "Force overwriting existing crates or binaries").short('f'))
        .arg_features()
        .arg(opt("debug", "Build in debug mode instead of release mode"))
        .arg_targets_bins_examples(
            "Install only the specified binary",
            "Install all binaries",
            "Install only the specified example",
            "Install all examples",
        )
        .arg_target_triple("Build for the target triple")
        .arg(opt("root", "Directory to install packages into").value_name("DIR"))
        .arg(opt("registry", "Registry to use").value_name("REGISTRY"))
        .after_help(
            "\
This command manages Cargo's local set of installed binary crates. Only packages
which have [[bin]] targets can be installed, and all binaries are installed into
the installation root's `bin` folder. The installation root is determined, in
order of precedence, by `--root`, `$CARGO_INSTALL_ROOT`, the `install.root`
configuration key, and finally the home directory (which is either
`$CARGO_HOME` if set or `$HOME/.cargo` by default).

There are multiple sources from which a crate can be installed. The default
location is crates.io but the `--git` and `--path` flags can change this source.
If the source contains more than one package (such as crates.io or a git
repository with multiple crates) the `<crate>` argument is required to indicate
which crate should be installed.

Crates from crates.io can optionally specify the version they wish to install
via the `--vers` flags, and similarly packages from git repositories can
optionally specify the branch, tag, or revision that should be installed. If a
crate has multiple binaries, the `--bin` argument can selectively install only
one of them, and if you'd rather install examples the `--example` argument can
be used as well.

By default cargo will refuse to overwrite existing binaries. The `--force` flag
enables overwriting existing binaries. Thus you can reinstall a crate with
`cargo install --force <crate>`.

Omitting the <crate> specification entirely will
install the crate in the current directory. That is, `install` is equivalent to
the more explicit `install --path .`.  This behaviour is deprecated, and no
longer supported as of the Rust 2018 edition.

If the source is crates.io or `--git` then by default the crate will be built
in a temporary target directory.  To avoid this, the target directory can be
specified by setting the `CARGO_TARGET_DIR` environment variable to a relative
path.  In particular, this can be useful for caching build artifacts on
continuous integration systems.",
        )
}

fn cli_run() -> Command {
    Command::new("run")
        .alias("r")
        .trailing_var_arg(true)
        .about("Run the main binary of the local package (src/main.rs)")
        .arg(Arg::new("args").action(ArgAction::Append))
        .arg_targets_bin_example(
            "Name of the bin target to run",
            "Name of the example target to run",
        )
        .arg_package("Package with the target to run")
        .arg_jobs()
        .arg_release("Build artifacts in release mode, with optimizations")
        .arg_features()
        .arg_target_triple("Build for the target triple")
        .arg_target_dir()
        .arg_manifest_path()
        .arg_message_format()
        .after_help(
            "\
If neither `--bin` nor `--example` are given, then if the package only has one
bin target it will be run. Otherwise `--bin` specifies the bin target to run,
and `--example` specifies the example target to run. At most one of `--bin` or
`--example` can be provided.

All the arguments following the two dashes (`--`) are passed to the binary to
run. If you're passing arguments to both Cargo and the binary, the ones after
`--` go to the binary, the ones before go to Cargo.
",
        )
}

fn cli_logcat() -> Command {
    Command::new("logcat")
        .alias("r")
        .about("Print Android log")
        .arg_message_format()
}

pub fn execute_build(options: &ArgMatches, cargo_gctx: &GlobalContext) -> cargo::CliResult {
    let root_manifest = options.root_manifest(&cargo_gctx)?;

    let workspace = Workspace::new(&root_manifest, &cargo_gctx)?;

    let mut android_config =
        config::load(&workspace, &options.get_one::<String>("package").cloned())?;
    android_config.release = options.get_flag("release");

    ops::build(&workspace, &android_config, &options)?;
    Ok(())
}

pub fn execute_install(options: &ArgMatches, cargo_gctx: &GlobalContext) -> cargo::CliResult {
    let root_manifest = options.root_manifest(&cargo_gctx)?;

    let workspace = Workspace::new(&root_manifest, &cargo_gctx)?;

    let mut android_config =
        config::load(&workspace, &options.get_one::<String>("package").cloned())?;
    android_config.release = !options.get_flag("debug");

    ops::install(&workspace, &android_config, &options)?;
    Ok(())
}

pub fn execute_run(options: &ArgMatches, cargo_gctx: &GlobalContext) -> cargo::CliResult {
    let root_manifest = options.root_manifest(&cargo_gctx)?;

    let workspace = Workspace::new(&root_manifest, &cargo_gctx)?;

    let mut android_config =
        config::load(&workspace, &options.get_one::<String>("package").cloned())?;
    android_config.release = options.get_flag("release");

    ops::run(&workspace, &android_config, &options)?;
    Ok(())
}

pub fn execute_logcat(options: &ArgMatches, cargo_gctx: &GlobalContext) -> cargo::CliResult {
    let root_manifest = options.root_manifest(&cargo_gctx)?;

    let workspace = Workspace::new(&root_manifest, &cargo_gctx)?;

    let android_config = config::load(&workspace, &options.get_one::<String>("package").cloned())?;

    drop(writeln!(workspace.gctx().shell().err(), "Starting logcat"));
    let adb = android_config.sdk_path.join("platform-tools/adb");
    ProcessBuilder::new(&adb).arg("logcat").exec()?;

    Ok(())
}
