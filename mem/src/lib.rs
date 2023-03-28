use std::{env::temp_dir, path::PathBuf, process::id};

#[ctor::ctor]
fn initialize() {
    let addr = unsafe { libc::malloc(MEM_BLOCK_SIZE) };
    std::fs::write(
        mem_block_info_file().with_extension(id().to_string()),
        (addr as usize).to_le_bytes(),
    )
    .unwrap();
}

pub fn mem_block_info_file() -> PathBuf {
    temp_dir().join(env!("CARGO_PKG_NAME"))
}

pub const MEM_BLOCK_SIZE: usize = 1024 * 8;
