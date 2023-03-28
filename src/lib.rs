//! Intercept is a lib based on `ptrace` that intercepts and modifies Linux system calls.
//! It currently only supports `x86_64` architecture.
//!
//! # Usage
//! Write a function whose signature is same as a syscall, and mark it as `#[syscall]`,
//! and you are done.
//!
//! ```rust
//! #[syscall]
//! fn openat(dfd: i32, mut filename: *const c_char, flags: i32, mode: i32) -> i32 {
//!     // do something before syscall, logging, changing arguments, etc.
//!     
//!     let ret = real!(dfd, filename, flags, mode);
//!
//!     // do something after syscall, modifing return value..
//! }
//! ```
//! See more detail in examples
//!
//! # Extra Info
//!
//! ## Memory in target
//! We use "LD_PRELOAD" trick to insert a so into target process to malloc extra memory
//! needed when modified a pointer argument which has larger length.
//!
//! ## Remove dependency libgcc_s.so.1
//! Some glibc released without `libgcc_s.so.1`, we removed this dependency using link
//! script "linker_without_libgcc.wrap".
//!
//! ## special handling of parameter "*const *const c_char"
//!
//! > This parameter is used in syscall like `execve`'s `const char *const argv[]`.
//!
//! We treat `*const *const c_char` as `*const c_char` for convenience. That's say, original
//! pointer to pointer has been converted to pointer's content after content. For example:
//!
//! #### original
//! this ptr's address is inside target process, can not be used directly.
//!
//! | ptr (*const *const c_char)    | ptr to ptr (*const c_char)    | content   |
//! | ---                           | ---                           | ---       |
//! | 0x12345678                    | 0x11111111                    | "aaaa\0"  |
//! |                               | 0x22222222                    | "bbbb\0"  |
//! |                               | 0x33333333                    | "cccc\0"  |
//! |                               | 0x44444444                    | "\0"      |
//! ### after converted
//! memory is reallocated in interceptor instance, so address changed.
//!
//! | ptr (*const c_char)   | content                   |
//! | ---                   | ---                       |
//! | 0x87654321            | "aaaa\0bbbb\0cccc\0\0"    |
//! ### usage
//! you can use helper function [`read_ptr_to_ptr`] to read content from converted ptr.
//! and use [`write_ptr_to_ptr`] to write back.
//!
use anyhow::Result;
use once_cell::sync::Lazy;
use paste::paste;
use pete::{Ptracer, Restart, Stop, Tracee};
pub use ptr::{read_ptr_to_ptr, write_ptr_to_ptr};
use ptr::{MayBePtr, Number, Ptr, Read, RemoteMem, Write};
use rand::Rng;
use std::{cell::RefCell, collections::HashMap, env::current_exe, process::Command, rc::Rc};
use syscall::{ReturnVariant, ReturnVariantWrapper, SysCall, SysCallWrapper};
/// A proc-macro that turns a rust fn into a syscall.
///
/// See more details in examples.
pub use syscall_attr::syscall;
use tracing::debug;

mod ptr;
#[doc(hidden)]
pub mod syscall;

/// Provide the main functionality for intercepting.
pub struct Interceptor {
    ptracer: Ptracer,
    syscalls: Vec<SysCallWrapper>,
    block_calls: HashMap<u64, u64>,
    contexts: Rc<RefCell<HashMap<String, PackedContext>>>,
    remote_mem: Rc<RefCell<Option<RemoteMem>>>,
}

struct PackedContext(
    Box<dyn Context>,
    Box<dyn Context>,
    Box<dyn Context>,
    Box<dyn Context>,
    Box<dyn Context>,
    Box<dyn Context>,
);

trait Context {}

impl<T> Context for MayBePtr<T> {}

impl Interceptor {
    /// create child process by specific a [`std::process::Command`]
    pub fn new(mut cmd: Command) -> Result<Self> {
        let mut ptracer = Ptracer::new();
        cmd.env(
            "LD_PRELOAD",
            current_exe()?.with_file_name("libinter_mem.so"),
        );
        let _child = ptracer.spawn(cmd)?;

        Ok(Self {
            ptracer,
            syscalls: Vec::new(),
            block_calls: HashMap::new(),
            contexts: Rc::new(RefCell::new(HashMap::new())),
            remote_mem: Rc::new(RefCell::new(None)),
        })
    }

    /// register syscall to interceptor
    pub fn on<R, A1, A2, A3, A4, A5, A6>(
        &mut self,
        syscall: &'static SysCall<R, A1, A2, A3, A4, A5, A6>,
    ) -> &mut Self
    where
        R: Number,
        A1: Read,
        A2: Read,
        A3: Read,
        A4: Read,
        A5: Read,
        A6: Read,
        MayBePtr<<A1 as Read>::InnerType>: Write<A1> + Ptr<A1>,
        MayBePtr<<A2 as Read>::InnerType>: Write<A2> + Ptr<A2>,
        MayBePtr<<A3 as Read>::InnerType>: Write<A3> + Ptr<A3>,
        MayBePtr<<A4 as Read>::InnerType>: Write<A4> + Ptr<A4>,
        MayBePtr<<A5 as Read>::InnerType>: Write<A5> + Ptr<A5>,
        MayBePtr<<A6 as Read>::InnerType>: Write<A6> + Ptr<A6>,
    {
        let contexts = self.contexts.clone();
        let remote_mem = self.remote_mem.clone();
        self.syscalls.push(SysCallWrapper {
            name: syscall.name,
            pre: Box::new(move |tracee, a1, a2, a3, a4, a5, a6| {
                let mut a1 = A1::read(tracee, a1);
                let mut a2 = A2::read(tracee, a2);
                let mut a3 = A3::read(tracee, a3);
                let mut a4 = A4::read(tracee, a4);
                let mut a5 = A5::read(tracee, a5);
                let mut a6 = A6::read(tracee, a6);
                match syscall.call_pre(a1.get(), a2.get(), a3.get(), a4.get(), a5.get(), a6.get()) {
                    ReturnVariant::PackedArgs((r1, r2, r3, r4, r5, r6)) => {
                        let pa = (
                            a1.write(tracee, remote_mem.clone(), r1),
                            a2.write(tracee, remote_mem.clone(), r2),
                            a3.write(tracee, remote_mem.clone(), r3),
                            a4.write(tracee, remote_mem.clone(), r4),
                            a5.write(tracee, remote_mem.clone(), r5),
                            a6.write(tracee, remote_mem.clone(), r6),
                        );
                        contexts.borrow_mut().insert(
                            syscall.name.to_string(),
                            PackedContext(
                                Box::new(a1),
                                Box::new(a2),
                                Box::new(a3),
                                Box::new(a4),
                                Box::new(a5),
                                Box::new(a6),
                            ),
                        );
                        ReturnVariantWrapper::PackedArgs(pa)
                    }
                    ReturnVariant::Normal(r) => ReturnVariantWrapper::Normal(r.to_u64()),
                }
            }),
            post: Box::new(|u| syscall.call_post(R::from_u64(u)).to_u64()),
        });
        self
    }

    /// run the child process and begin intercepting
    pub fn run(&mut self) -> Result<()> {
        while let Some(mut tracee) = self.ptracer.wait()? {
            self.on_stop(&mut tracee)?;
            self.ptracer.restart(tracee, Restart::Syscall)?;
        }

        Ok(())
    }

    fn on_stop(&mut self, tracee: &mut Tracee) -> Result<()> {
        let mut regs = tracee.registers()?;
        let pc = regs.rip;
        let Tracee { pid, stop, .. } = tracee;

        match stop {
            Stop::SyscallEnter => {
                let syscall = SYSCALL_TABLE
                    .get(&regs.orig_rax)
                    .cloned()
                    .unwrap_or_else(|| format!("unknown (syscall no = 0x{:x})", regs.orig_rax));
                debug!(
                    "pid = {}, pc = {:x}: [{}] {:?}\nregs: {:x?}",
                    pid, pc, syscall, stop, regs
                );

                if let Some(sc) = self.syscalls.iter_mut().find(|sc| sc.name == syscall) {
                    match (sc.pre)(
                        tracee, regs.rdi, regs.rsi, regs.rdx, regs.r10, regs.r8, regs.r9,
                    ) {
                        ReturnVariantWrapper::PackedArgs((r1, r2, r3, r4, r5, r6)) => {
                            macro_rules! set_reg {
                                ($r:path ,$n: tt) => {
                                    paste! {
                                        if let Some([<r $n>]) = [<r $n>] {
                                            regs.$r = [<r $n>];
                                        }
                                    }
                                };
                            }

                            set_reg!(rdi, 1);
                            set_reg!(rsi, 2);
                            set_reg!(rdx, 3);
                            set_reg!(r10, 4);
                            set_reg!(r8, 5);
                            set_reg!(r9, 6);
                            tracee.set_registers(regs)?;
                            self.contexts.borrow_mut().remove(&syscall);
                        }
                        ReturnVariantWrapper::Normal(r) => {
                            // syscall will be blocked, call a non-exists & random sysno,
                            let sysno = 512 + rand::thread_rng().gen::<u16>() as u64;
                            self.block_calls.insert(sysno, r);
                            debug!(
                                "block call change sysno {} -> {}. ret: {}",
                                regs.orig_rax, sysno, r
                            );
                            regs.orig_rax = sysno;
                            tracee.set_registers(regs)?;
                        }
                    }
                }
            }
            Stop::SyscallExit => {
                if let Some(block_call_ret) = self.block_calls.remove(&regs.orig_rax) {
                    debug!(
                        "block call sysno: {}, ret: {}",
                        regs.orig_rax, block_call_ret
                    );
                    regs.rax = block_call_ret;
                    tracee.set_registers(regs)?;
                } else {
                    let syscall = SYSCALL_TABLE
                        .get(&regs.orig_rax)
                        .cloned()
                        .unwrap_or_else(|| format!("unknown (syscall no = 0x{:x})", regs.orig_rax));
                    debug!(
                        "pid = {}, pc = {:x}: [{}] {:?}\nregs: {:x?}",
                        pid, pc, syscall, stop, regs
                    );

                    if let Some(sc) = self.syscalls.iter_mut().find(|sc| sc.name == syscall) {
                        let ret = (sc.post)(regs.rax);
                        regs.rax = ret;
                        tracee.set_registers(regs)?;
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }
}

/// A fake macro that actually does nothing.
/// It will be detected in `proc_macro_attribute` and changes intercept logic.
#[macro_export]
macro_rules! real {
    ($($args: tt),*) => {
        let _ = ($($args),*);
    };
}

type SyscallTable = HashMap<u64, String>;
static SYSCALL_TABLE: Lazy<SyscallTable> = Lazy::new(load_syscall_table);
const SYSCALLS: &str = include_str!("data/syscalls_x64.tsv");

fn load_syscall_table() -> SyscallTable {
    let mut syscalls = HashMap::new();

    for line in SYSCALLS.split_terminator('\n') {
        let (call_no, name) = line
            .split_once('\t')
            .map(|(x, y)| (x.trim().parse::<u64>().unwrap(), y.trim().to_owned()))
            .unwrap();
        syscalls.insert(call_no, name);
    }

    syscalls
}
