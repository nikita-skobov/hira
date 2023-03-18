use std::{process::Command, path::PathBuf};

fn dir_exists(start_dir: &str, check_dir: &str) -> Result<bool, String> {
    let readdir = std::fs::read_dir(start_dir).map_err(|e| e.to_string())?;

    for d in readdir {
        let d = d.map_err(|e| e.to_string())?;
        let filename = d.file_name().to_string_lossy().to_string();
        if filename == check_dir {
            return Ok(true);
        }
    }
    Ok(false)
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn compare_files(
    old: &str,
    new: &str,
) -> Result<(), String> {
    let cmd = Command::new("git").arg("--no-pager").arg("diff").arg("--no-index")
        .arg("--").arg(&old).arg(&new).output().map_err(|e| format!("Failed to run git diff on {} and {}\n{e}", old, new))?;
    // comparison succeeded: files are the same.
    if cmd.status.success() {
        return Ok(())
    }
    let err = String::from_utf8_lossy(&cmd.stderr).to_string();
    let err2 = String::from_utf8_lossy(&cmd.stdout).to_string();
    Err(format!("{new} failed snapshot test!\n{err}\n{err2}\n\nIf this change is expected, re-run the testing program with --update {new}"))
}

/// files are the files you wish to snapshot from that example directory.
/// so for example in `examples/hello_world/` there is a `deploy.sh` file
/// so you would provide: example_dir: `"hello_world"` and files: `&["deploy.sh"]`
fn write_snapshot(
    example_dir: &str,
    files: &[&str],
    updates: &Vec<String>,
) -> Result<(), String> {
    println!("Running cargo build for examples/{example_dir}");
    let cmd = Command::new("cargo").arg("build")
        .current_dir(&format!("./examples/{example_dir}/"))
        .output().map_err(|e| e.to_string())?;
    if !cmd.status.success() {
        let err = String::from_utf8_lossy(&cmd.stderr).to_string();
        return Err(format!("Failed to run cargo build:\n{err}"));
    }

    for file in files {
        let file_path = format!("./examples/{example_dir}/{file}");
        let to = format!("./testing/snapshots/{example_dir}/{file}");
        let mut to_path = PathBuf::from(&to);
        to_path.pop();
        std::fs::metadata(&file_path).map_err(|e| format!("Invalid snapshot testing invocation. Src file {file_path} does not exist.\n{e}"))?;
        std::fs::create_dir_all(&to_path).map_err(|e| format!("Failed to create directory {:?}\n{e}", to_path))?;
        // if the destination file exists, compare against the src file.
        // if it doesn't exist, just copy it over.
        let dest_exists = match std::fs::metadata(&to) {
            Ok(_) => true,
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => false,
                _ => return Err(format!("Error checking validity of destination {to}\n{e}")),
            }
        };
        if dest_exists {
            // if user said to update this file, just update the snapshot:
            if !updates.contains(&file_path) {
                compare_files(&to, &file_path)?;
                // if successful, output a log :)
                println!("✓ {file_path}");
            } else {
                println!("Updating {file_path}");
            }
        } else {
            println!("New {file_path}");
        }
        // if the comparison succeeds, or if this is a new file, then copy it over:
        std::fs::copy(&file_path, &to).map_err(|e| format!("Error copying {} to {}. {}", file_path, to, e.to_string()))?;
    }

    Ok(())
}

fn run_snapshots(should_override: bool, updates: Vec<String>) -> Result<(), String> {
    if should_override {
        // ignore this error if it fails
        let _ = std::fs::remove_dir_all("./testing/snapshots/");
        // this must succeed: error if it doesnt.
        std::fs::create_dir("./testing/snapshots/").map_err(|e| e.to_string())?;
    }

    // iterate over the examples directory:
    for example_dir in std::fs::read_dir("./examples").map_err(|e| e.to_string())? {
        let example_dir = example_dir.map_err(|e| e.to_string())?;
        let example_dir_file_name = example_dir.file_name().to_string_lossy().to_string();
        write_snapshot(&example_dir_file_name, &["deploy.sh", "hira/deploy.yml"], &updates)?;
    }

    Ok(())
}


fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut updates = vec![];
    let mut next_is_update = false;
    let mut update_all = false;
    for arg in args {
        if next_is_update {
            updates.push(arg);
            next_is_update = false;
            continue;
        }
        if arg == "--update-all" {
            update_all = true;
            break;
        }
        if arg == "--update" {
            next_is_update = true;
        }
    }
    let examples_exists = dir_exists(".", "examples")?;
    let testing_exists = dir_exists(".", "testing")?;
    if !examples_exists || !testing_exists {
        return Err(format!("Did not find examples/ and testing/ directory. Are you running this from the root of the hira directory?"));
    }
    let mut should_overwrite = if !dir_exists("./testing", "snapshots")? {
        println!("No snapshots yet. will create snapshots and exit");
        true
    } else {
        false
    };
    if update_all {
        should_overwrite = true;
    }
    run_snapshots(should_overwrite, updates)?;

    Ok(())
}
