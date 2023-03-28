use interceptor_rs::{syscall, Interceptor};
use std::{
    env::args,
    ffi::{c_char, CStr, CString},
    mem::forget,
    process::Command,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = args().collect::<Vec<_>>();
    let mut cmd = Command::new(&args[0]);
    if let Some(args) = args.get(1..) {
        cmd.args(args);
    }
    Interceptor::new(cmd)?.on(&openat).run()?;
    Ok(())
}

#[syscall]
fn openat(dfd: i32, mut filename: *const c_char, flags: i32, mode: i32) -> i32 {
    let file = unsafe { CStr::from_ptr(filename).to_string_lossy() };
    println!("openat filename: {}", file);
    if file == "1.c" {
        {
            // If you want to change the content of the filename pointer, make sure that
            // the length of the changed content is less than or equal to the original
            // length, otherwise the extra bytes will be truncated.
            let a = b"2.c";
            unsafe {
                std::ptr::copy_nonoverlapping(a.as_ptr(), filename as *mut u8, a.len());
            }
        }

        {
            // Alternatively, you can apply for a string memory yourself, and then change
            // the filename pointer itself.
            let a = CString::new("1.cpp").unwrap();
            filename = a.as_ptr() as *mut c_char;
            // In this case, don't forget to call `forget` to make sure that the content
            // pointed by the return pointer is not released.
            forget(a);
        }
    }

    // call the `real!()` macro to pass the modified arguments to the kernel, and of course
    // you can alse modify its return value. If you don't call the `real!()` macro, the
    // original system call will not be called in kernel (here is `openat`), and the result
    // will be returned directly to the caller of the original caller.
    let ret = real!(dfd, filename, flags, mode);
    println!("ret: {}", ret);
    ret
}
