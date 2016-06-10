extern crate cargo;
extern crate docopt;
extern crate git2;
extern crate walkdir;
extern crate libc;
extern crate rustc_serialize;

use std::env;
use std::fs::{self, File};
use std::io::prelude::*;
use std::io;
use std::os::unix::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

use cargo::core::{Source, SourceId, Registry, Dependency};
use cargo::ops;
use cargo::sources::RegistrySource;
use cargo::core::shell::{Shell, MultiShell, Verbosity, ShellConfig, ColorConfig};
use cargo::util::Config;
use docopt::Docopt;
use rustc_serialize::Decodable;
use walkdir::{WalkDir, DirEntry, WalkDirIterator};

// Write the Docopt usage string.
const USAGE: &'static str = r#"
Usage:
  time_cargo build [options] <package-name>...
  time_cargo test [options] <package-name>...

Builds or tests the latest version of packages from crates.io, saving
timing information and other results. If the special package-name "*"
is used, we will test all packages. (Use `'*'` to prevent your shell
from expanding wildcards.)

WARNING: Building or testing packages from crates.io involves executing
arbitary code! Be wary.

Options:
  -h, --help  Show this screen.
  -o DIR      Output directory [default: out].
"#;

#[derive(Debug, RustcDecodable)]
struct Args {
    cmd_build: bool,
    cmd_test: bool,
    flag_help: bool,
    flag_o: String,
    arg_package_name: Vec<String>,
}

fn main() {
    let args: Args =
        Docopt::new(USAGE)
        .and_then(|d| d.argv(env::args()).decode())
        .unwrap_or_else(|e| e.exit());

    let root = PathBuf::from(args.flag_o.clone());

    let index = root.join("index");
    if fs::metadata(&index).is_err() {
        let dot_index = root.join(".index");
        git2::Repository::clone("https://github.com/rust-lang/crates.io-index",
                                &dot_index).unwrap();
        fs::rename(&dot_index, &index).unwrap();
    }

    let config = config();
    let id = SourceId::for_central(&config).unwrap();
    let mut s = RegistrySource::new(&id, &config);
    s.update().unwrap();

    let stdout = unsafe { libc::dup(1) };
    let stderr = unsafe { libc::dup(2) };
    assert!(stdout > 0 && stderr > 0);

    let krate_names = if args.arg_package_name.iter().any(|s| s == "*") {
        WalkDir::new(&index)
            .into_iter()
            .filter_entry(|e| !bad(e))
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.file_name().to_str().unwrap().to_string())
            .collect()
    } else {
        args.arg_package_name.clone()
    };

    for krate in krate_names {
        //FIXME: Skip crates this script has trouble with, but ultimately
        //we should figure out why and include them
        if krate == "cosmo" { continue; }
        if krate == "gfx_text" { continue; }
        if krate == "parasailors" { continue; }
        if krate == "parasail-sys" { continue; }
        if krate == "simple" { continue; }
        let root_output = root.join("output").join(&krate);
        if fs::metadata(root.join("output").join(&krate).join("stdio")).is_ok() {
            println!("skipping {}", krate);
            continue
        }
        build_or_test(&args, &root, &root_output, &mut s, &id, &krate);
        io::stdout().flush().unwrap();
        unsafe {
            assert_eq!(libc::dup2(stdout, 1), 1);
            assert_eq!(libc::dup2(stderr, 2), 2);
        }
    }
}

fn bad(entry: &DirEntry) -> bool {
    entry.file_name()
         .to_str()
         .map(|s| s.starts_with(".") || s.ends_with(".json"))
         .unwrap_or(false)
}

fn config() -> Config {
    let config = ShellConfig {
        color_config: ColorConfig::Always,
        tty: true,
    };
    let out = Shell::create(Box::new(io::stdout()), config);
    let err = Shell::create(Box::new(io::stderr()), config);
    Config::new(MultiShell::new(out, err, Verbosity::Normal),
        env::current_dir().unwrap(), env::home_dir().unwrap()).unwrap()
}

fn build_or_test(args: &Args,
                 root: &Path,
                 out: &Path,
                 src: &mut RegistrySource,
                 id: &SourceId,
                 krate: &str) {
    println!("working on: {}", krate);
    fs::create_dir_all(&out).unwrap();
    unsafe {
        let stdout = File::create(out.join("stdio")).unwrap();
        assert_eq!(libc::dup2(stdout.as_raw_fd(), 1), 1);
        assert_eq!(libc::dup2(stdout.as_raw_fd(), 2), 2);
    }

    let dep = Dependency::parse(krate, None, &id).unwrap();
    let pkg = src.query(&dep).unwrap().iter().map(|v| v.package_id())
                 .max().cloned();
    let pkg = match pkg {
        Some(pkg) => pkg,
        None => {
            return println!("failed to find {}", krate);
        }
    };

    let pkg = match src.download(&pkg) {
        Ok(v) => v,
        Err(e) => {
            return println!("bad get pkg: {}: {}", pkg, e);
        }
    };

    fs::create_dir_all(".cargo").unwrap();
    File::create(".cargo/config").unwrap().write_all(format!("
        [build]
        target-dir = '{}'
    ", root.join("results").display()).as_bytes()).unwrap();

    let rustc_args = &["-Z".to_string(), "time-passes".to_string()];

    let compile_opts = ops::CompileOptions {
        config: &config(),
        jobs: None,
        target: None,
        features: &[],
        no_default_features: false,
        spec: &[],
        filter: ops::CompileFilter::Only {
            lib: true,
            examples: &[],
            bins: &[],
            tests: &[],
            benches: &[],
        },
        exec_engine: None,
        release: true,
        mode: ops::CompileMode::Build,
        target_rustc_args: Some(rustc_args),
        target_rustdoc_args: None
    };

    match ops::compile_pkg(&pkg, None, &compile_opts) {
        Ok(_) => println!("> compile passed for `{}`", pkg),
        Err(e) => println!("> compile failed for `{}`: {}", pkg, e)
    }

    if args.cmd_test {
        let test_opts = ops::TestOptions {
            compile_opts: compile_opts,
            no_run: false,
            no_fail_fast: false
        };

        let start = Instant::now();
        let result = ops::run_tests(pkg.manifest_path(), &test_opts, &[]);
        let test_time = start.elapsed();

        match result {
            Ok(None) => println!("> tests passed for `{}`: {:?}", pkg, test_time),
            Ok(Some(err)) => println!("> tests failed for `{}`: {}", pkg, err),
            Err(err) => println!("> cargo error for `{}`: {}", pkg, err),
        }
    }
}

