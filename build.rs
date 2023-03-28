use std::{
    env::{temp_dir, var},
    fs::copy,
    path::{Path, PathBuf},
    process::Command,
};

fn find_target_dir(out_dir: &Path) -> Option<PathBuf> {
    for ancestor in out_dir.ancestors() {
        let read_dir = ancestor
            .read_dir()
            .expect(&format!("can not read dir for {}", ancestor.display()));
        for entry in read_dir {
            if let Ok(f) = entry {
                if f.file_name() == ".cargo-lock" {
                    return Some(ancestor.to_path_buf());
                }
            }
        }
    }

    None
}

const INTER_MEM_NAME: &str = "libinter_mem.so";

fn main() {
    println!("cargo:rerun-if-changed=mem");

    if !Path::new("mem").exists() {
        return;
    }

    let out_dir = var("OUT_DIR").expect("env OUT_DIR not found");
    let out_dir = Path::new(&out_dir);
    let target_dir = find_target_dir(out_dir).expect("can not found target dir");

    let temp_dir = temp_dir()
        .join(INTER_MEM_NAME)
        .join("target")
        .to_string_lossy()
        .to_string();
    let target = var("TARGET").expect("env TARGET not found");
    let profile = var("PROFILE").expect("env PROFILE not found");
    let mut result_dir = Path::new(&temp_dir).join(&target);

    let mut cmd = Command::new("cargo");
    cmd.current_dir("mem")
        .args(["build", "--target", &target, "--target-dir", &temp_dir]);
    result_dir = if profile != "debug" {
        cmd.arg(format!("--{profile}"));
        result_dir.join(profile)
    } else {
        result_dir.join("debug")
    };

    let output = cmd.output().unwrap();
    if !output.status.success() {
        panic!(
            "intercept generate {} error:\n{}",
            INTER_MEM_NAME,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    copy(
        result_dir.join(INTER_MEM_NAME),
        target_dir.join(INTER_MEM_NAME),
    )
    .expect(&format!("copy result {} error", INTER_MEM_NAME));
}
