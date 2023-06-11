use std::{path::{Path, PathBuf}, process::{Command, Stdio}};
use hira_lib::{HiraConfig, parsing::{iter_hira_modules, get_ident_string}, module_loading::print_debug, level0::RuntimeMeta};
use quote::ToTokens;


fn iter_files_recursively<P: AsRef<Path>>(
    start_dir: P,
    callback: &mut impl FnMut(PathBuf) -> Result<(), String>,
) -> Result<(), String> {
    let readdir = std::fs::read_dir(start_dir.as_ref())
        .map_err(|e| format!("Failed to read dir {:?}\n{:?}", start_dir.as_ref(), e))?;
    for entry in readdir {
        let direntry = entry.map_err(|e| format!("Failed to get readdir entry from {:?}\n{:?}", start_dir.as_ref(), e))?;
        let path = direntry.path();
        let fp = direntry.file_type().map_err(|e| format!("Failed to get file type from {:?}\n{:?}", path, e))?;
        if fp.is_dir() {
            iter_files_recursively(&path, callback)?;
        } else {
            callback(path)?;
        }
    }
    Ok(())
}

/// given a search dir, see if Cargo.toml exists in this dir,
/// and if so, return the dir that contains Cargo.toml (not the path to the file,
/// the path to the dir). If not found, back up 1 dir at a time
/// until a Cargo.toml file is found (limit 5 times)
fn find_closest_cargo_toml(mut search_dir: PathBuf) -> Option<PathBuf> {
    // TODO: make this configurable
    for _ in 0..5 {
        search_dir.push("Cargo.toml");
        if std::fs::File::open(&search_dir).is_ok() {
            search_dir.pop();
            return Some(search_dir);
        }
        search_dir.pop();
        search_dir.pop();
    }
    None
}

fn main() {
    let compile_and_run_runtime = std::env::args().nth(1);
    let cargo_home = env!("CARGO_HOME");
    std::env::set_var("CARGO_HOME", cargo_home);
    let currdir = std::env::current_dir().expect("Failed to get current directory");
    let manifest_dir = match find_closest_cargo_toml(currdir.clone()) {
        Some(o) => o,
        None => {
            eprintln!("Failed to find Cargo.toml from {:?}. Ensure you are running this from a cargo project", currdir);
            std::process::exit(1);
        }
    };
    std::env::set_var("CARGO_MANIFEST_DIR", manifest_dir);
    println!("Scanning all rust files from {:?}", currdir);
    let mut all_rust_files = vec![];
    let res = iter_files_recursively(&currdir, &mut |p| {
        if let Some(ext) = p.extension() {
            let ext = ext.to_string_lossy().to_string();
            if ext == "rs" {
                all_rust_files.push(p);
            }
        }
        Ok(())
    });
    if let Err(e) = res {
        eprintln!("{e}");
        std::process::exit(1);
    }
    let mut conf = match fill_hira_graph(&all_rust_files) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    if let Err(e) = build_runtimes(&mut conf, compile_and_run_runtime) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn build_runtimes(
    conf: &mut HiraConfig,
    compile_and_run_runtime: Option<String>
) -> Result<(), String> {
    let logfile = conf.logfile.clone();
    // if a runtime was specified, compile only that runtime and then run it.
    // otherwise, simply compile all runtimes, but dont run them.
    if let Some(runtime_name) = compile_and_run_runtime {
        // first compile all dependencies of this runtime
        if let Some(dependencies) = conf.runtime_depends_on.get(&runtime_name) {
            for dep_name in dependencies.iter() {
                if let Some((_, runtime, _, _)) = conf.runtimes.get(dep_name) {
                    build_runtime(
                        &dep_name, &conf.wasm_directory,
                        &conf.runtime_directory, &conf.crate_name,
                        runtime, &logfile, Some(&runtime_name),
                    )?;
                } else {
                    return Err(format!("Failed to find dependency {} on runtime {}", dep_name, runtime_name));
                }
            }
        }
        if let Some((_, runtime, _, _)) = conf.runtimes.get(&runtime_name) {
            // now build and run the requested runtime.
            build_runtime(
                &runtime_name, &conf.wasm_directory,
                &conf.runtime_directory, &conf.crate_name,
                runtime, &logfile, None,
            )?;
            run_runtime(&conf.runtime_directory, &runtime_name)?;
        } else {
            return Err(format!("Runtime {} does not exist", runtime_name));
        }
    } else {
        // otherwise just build all runtimes but dont run them.
        for (name, (_, runtime, _, _)) in conf.runtimes.iter() {
            build_runtime(
                &name, &conf.wasm_directory,
                &conf.runtime_directory, &conf.crate_name,
                runtime, &logfile, None
            )?;
        }
    }
    Ok(())
}

fn run_runtime(
    runtime_dir: &str,
    name: &str,
) -> Result<(), String> {
    let cmd = format!("{runtime_dir}/{name}");
    let out = Command::new(&cmd)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdin(Stdio::null())
        .output().map_err(|e| format!("Failed to invoke {cmd}\n{:?}", e))?;
    if !out.status.success() {
        return Err(format!("Failed to run runtime {name}"));
    }

    Ok(())
}

fn build_runtime(
    name: &str,
    wasm_dir: &str,
    runtime_dir: &str,
    crate_name: &str,
    runtime: &RuntimeMeta,
    logfile: &str,
    dependency_of: Option<&String>,
) -> Result<(), String> {
    let target_dir = format!("{}/target_{}", wasm_dir, name);
    let hira_runtime_output_path = format!("{}/{}", runtime_dir, name);
    if let Some(dep) = &dependency_of {
        println!("Building runtime {name} (dependency of {dep})");
    } else {
        println!("Building runtime {name}");
    }
    let now = std::time::Instant::now();
    HiraConfig::run_build_runtime_cmd(runtime, &name, &target_dir, crate_name, &hira_runtime_output_path)?;
    let elapsed = now.elapsed().as_millis();
    let contents = format!("Building {name}, dur={elapsed}ms\n");
    print_debug(logfile, &contents);
    Ok(())
}

fn compile_log(name: &str) -> String {
    format!("Analyzing {name}")
}

fn fill_hira_graph(files: &Vec<PathBuf>) -> Result<HiraConfig, String> {
    let mut conf = HiraConfig::new();
    conf.should_do_file_ops = true;
    conf.should_output_build_script = false;
    let logfile = conf.logfile.clone();
    for f in files.iter() {
        let contents = std::fs::read_to_string(f)
            .map_err(|e| format!("Failed to read file {:?}\n{:?}", f, e))?;
        iter_hira_modules(&contents, &mut |m| {
            if !hira_lib::parsing::has_attr_that_ends_in(&m.attrs, "hira") {
                return Ok(true);
            }
            let tokens = m.to_token_stream();
            let ident = get_ident_string(&m.ident);
            let now = std::time::Instant::now();
            hira_lib::module_loading::hira_mod2_inner_ex(
                &mut conf, tokens, true,
                false, None, Some(compile_log))?;
            let elapsed = now.elapsed().as_millis();
            let contents = format!("Analyzing {ident}, dur={elapsed}ms\n");
            print_debug(&logfile, &contents);
            Ok(true)
        }).map_err(|e| format!("Failed to get hira modules from {:?}\n{:?}", f, e))?;
    }
    Ok(conf)
}
