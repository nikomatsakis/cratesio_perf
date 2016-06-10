extern crate cargo;
extern crate git2;
extern crate walkdir;
extern crate libc;

use std::env;
use std::fs::{self, File};
use std::io::prelude::*;
use std::io;
use std::os::unix::prelude::*;
use std::path::Path;

use cargo::core::{Source, SourceId, Registry, Dependency};
use cargo::ops;
use cargo::sources::RegistrySource;
use cargo::core::shell::{Shell, MultiShell, Verbosity, ShellConfig, ColorConfig};
use cargo::util::Config;
use walkdir::{WalkDir, DirEntry, WalkDirIterator};

fn main() {
    if fs::metadata("index").is_err() {
        git2::Repository::clone("https://github.com/rust-lang/crates.io-index",
                                ".index").unwrap();
        fs::rename(".index", "index").unwrap();
    }

    let config = config();
    let id = SourceId::for_central(&config).unwrap();
    let mut s = RegistrySource::new(&id, &config);
    s.update().unwrap();

    let stdout = unsafe { libc::dup(1) };
    let stderr = unsafe { libc::dup(2) };
    assert!(stdout > 0 && stderr > 0);

    let root = env::current_dir().unwrap();
    for krate in WalkDir::new("index").into_iter()
                        .filter_entry(|e| !bad(e))
                        .filter_map(|e| e.ok())
                        .filter(|e| e.file_type().is_file())
                        .map(|e| e.file_name().to_str().unwrap().to_string()) {
        //FIXME: Skip crates this script has trouble with, but ultimately
        //we should figure out why and include them
        if krate == "cosmo" { continue; }
        if krate == "gfx_text" { continue; }
        if krate == "parasailors" { continue; }
        if krate == "parasail-sys" { continue; }
        if krate == "simple" { continue; }
        let root_output = root.join("output").join(&krate);
        if fs::metadata(root.join("output").join(&krate).join("stdio")).is_ok() {
            continue
        }
        build(&root_output, &mut s, &id, &krate);
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

fn build(out: &Path, src: &mut RegistrySource, id: &SourceId, krate: &str) {
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
        Ok(mut v) => v,
        Err(e) => {
            return println!("bad get pkg: {}: {}", pkg, e);
        }
    };

    fs::create_dir_all(".cargo").unwrap();
    File::create(".cargo/config").unwrap().write_all("
        [build]
        target-dir = '/home/ec2-user/versioned/cratesio_perf/time_cargo/results'
    ".as_bytes()).unwrap();

    let config = config();
    let args = &["-Z".to_string(), "time-passes".to_string()];
    let res = ops::run_tests(&pkg, None, &ops::TestOptions {
        compile_opts: CompileOptions {
            config: &config,
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
            target_rustc_args: Some(args),
            target_rustdoc_args: None
        },
        no_run: false,
        no_fail_fast: false,
        ony_doc: false});
    fs::remove_file(".cargo/config").unwrap();
    if let Err(e) = res {
        println!("bad compile {}: {} {:?}", pkg, e, e.cause());
    } else {
        println!("OK");
    }
}

