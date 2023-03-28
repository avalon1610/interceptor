use pete::Tracee;
use std::{
    cell::RefCell,
    ffi::{c_char, CStr, CString},
    fs::read,
    mem::size_of,
    rc::Rc,
    thread::sleep,
    time::Duration,
};
use tracing::warn;

pub struct RemoteMem {
    base: usize,
    offset: usize,
    max: usize,
}

impl RemoteMem {
    fn new(pid: i32) -> Self {
        let mut retry = 5;
        loop {
            match read(inter_mem::mem_block_info_file().with_extension(pid.to_string()))
                .map(|b| usize::from_le_bytes(b.try_into().unwrap_or_default()))
            {
                Err(e) => {
                    if retry >= 0 {
                        warn!("remote memory not ready try again, error: {:?}", e);
                        retry -= 1;
                        sleep(Duration::from_millis(50));
                        continue;
                    } else {
                        panic!("remote memory can not setup");
                    }
                }
                Ok(base) => {
                    return Self {
                        base,
                        offset: 0,
                        max: inter_mem::MEM_BLOCK_SIZE,
                    };
                }
            }
        }
    }
}

pub trait Read {
    type InnerType;

    fn read(remote: &mut Tracee, u: u64) -> MayBePtr<Self::InnerType>;
}

/// help to read content from ptr to ptr
pub fn read_ptr_to_ptr(p: *const *const c_char) -> Vec<Vec<u8>> {
    let mut offset = 0usize;
    let mut result = Vec::new();
    let mut buf = Vec::new();
    loop {
        let b: u8 = unsafe { std::ptr::read((p as usize + offset) as *const u8) };
        if buf.is_empty() && b == 0 {
            break;
        }

        buf.push(b);
        if b == 0 {
            result.push(buf);
            buf = Vec::new();
        }

        offset += 1;
    }

    result
}

/// help to write content back to ptr to ptr
pub fn write_ptr_to_ptr(p: *const *const c_char, v: Vec<Vec<u8>>) {
    let addr = p as usize;
    let mut offset = 0usize;
    v.into_iter().for_each(|x| unsafe {
        std::ptr::copy_nonoverlapping(x.as_ptr() as *const u8, (addr + offset) as *mut u8, x.len());
        offset += x.len()
    });
}

pub trait Number {
    fn from_u64(u: u64) -> Self;
    fn to_u64(self) -> u64;
}

impl Read for *const *const c_char {
    type InnerType = Vec<u8>;

    fn read(remote: &mut Tracee, u: u64) -> MayBePtr<Self::InnerType> {
        let mut mbp = MayBePtr {
            inner: Vec::new(),
            origin: u,
        };

        let mut offset = 0usize;
        loop {
            let mut buf = vec![0; size_of::<u64>()];
            let n = remote
                .read_memory_mut(u + offset as u64, &mut buf)
                .unwrap_or_default();
            let ptr = &buf[..n];
            offset += size_of::<u64>();
            let pdata =
                remote.read_bytes_with_nul(u64::from_le_bytes(ptr.try_into().unwrap_or_default()));
            if pdata.is_empty() {
                mbp.inner.push(b'\0');
                break;
            }

            mbp.inner.extend(pdata);
        }

        mbp
    }
}

trait LendingIterator {
    type Item<'a>
    where
        Self: 'a;
    fn next<'a>(&'a mut self) -> Option<Self::Item<'a>>;
}

struct MayBePtrIter<'a> {
    offset: usize,
    inner: &'a MayBePtr<Vec<u8>>,
}

impl MayBePtr<Vec<u8>> {
    fn iter(&self) -> MayBePtrIter {
        MayBePtrIter {
            offset: 0,
            inner: self,
        }
    }
}

impl<'a> LendingIterator for MayBePtrIter<'a> {
    type Item<'i> = &'i [u8] where Self: 'i;

    fn next<'i>(&'i mut self) -> Option<Self::Item<'i>> {
        let mut next = 0;
        while let Some(v) = self.inner.inner.get(self.offset + next) {
            if *v == 0 {
                let offset = self.offset;
                self.offset += next + 1;
                return Some(&self.inner.inner[offset..=offset + next]);
            }

            next += 1;
        }

        None
    }
}

impl Write<*const *const c_char> for MayBePtr<Vec<u8>> {
    fn write(
        &mut self,
        remote: &mut Tracee,
        _remote_mem: Rc<RefCell<Option<RemoteMem>>>,
        v: Option<*const *const c_char>,
    ) -> Option<u64> {
        if let Some(v) = v {
            if self.inner.as_ptr() != v as *const u8 {
                panic!("*const *const c_char doesn't support change pointer");
            }

            let mut offset = 0usize;
            let mut iter = self.iter();
            loop {
                let mut buf = vec![0; size_of::<u64>()];
                let n = remote
                    .read_memory_mut(self.origin + offset as u64, &mut buf)
                    .unwrap_or_default();
                let ptr = &buf[..n];
                offset += size_of::<u64>();
                let addr = u64::from_le_bytes(ptr.try_into().unwrap_or_default());
                if addr != 0 {
                    let next = iter.next();
                    if let Some(next) = next {
                        remote
                            .write_memory(addr, next)
                            .expect("write remote memory for ptr to ptr error");
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            Some(self.origin)
        } else {
            None
        }
    }
}

impl Ptr<*const *const c_char> for MayBePtr<Vec<u8>> {
    fn get(&self) -> *const *const c_char {
        self.inner.as_ptr() as *const *const c_char
    }
}

trait ReadRemote {
    fn read_bytes_with_nul(&mut self, addr: u64) -> Vec<u8>;
}

impl ReadRemote for Tracee {
    fn read_bytes_with_nul(&mut self, addr: u64) -> Vec<u8> {
        let mut data = Vec::new();
        if addr != 0 {
            let mut offset = 0usize;
            loop {
                let mut buf = vec![0; size_of::<u64>()];
                let n = self
                    .read_memory_mut(addr + offset as u64, &mut buf)
                    .unwrap_or_default();
                if n == 0 {
                    break;
                }
                let buf = &buf[..n];
                if let Some(i) = buf.iter().position(|x| *x == b'\0') {
                    data.extend(&buf[..=i]);
                    break;
                }

                offset += size_of::<u64>();
                data.extend(buf);
            }
        }
        data
    }
}

macro_rules! ptr_impl {
    ($t: ty) => {
        impl Read for $t {
            type InnerType = Vec<u8>;

            fn read(remote: &mut Tracee, u: u64) -> MayBePtr<Vec<u8>> {
                MayBePtr {
                    inner: remote.read_bytes_with_nul(u),
                    origin: u,
                }
            }
        }

        impl Ptr<$t> for MayBePtr<Vec<u8>> {
            fn get(&self) -> $t {
                self.inner.as_ptr() as $t
            }
        }

        impl Write<$t> for MayBePtr<Vec<u8>> {
            fn write(
                &mut self,
                remote: &mut Tracee,
                remote_mem: Rc<RefCell<Option<RemoteMem>>>,
                v: Option<$t>,
            ) -> Option<u64> {
                if let Some(v) = v {
                    if self.inner.as_ptr() == v as *const u8 {
                        // origin inner's pointer not changed by argument
                        remote
                            .write_memory(self.origin, &self.inner)
                            .expect("write origin memory error");
                        Some(self.origin)
                    } else {
                        // pointer changed, meaning user allocate new memory in rust
                        let c = unsafe { CStr::from_ptr(v).to_bytes_with_nul() };
                        let remote_addr = alloc_remote_mem(remote, remote_mem, c.len()) as u64;
                        remote
                            .write_memory(remote_addr, &c)
                            .expect("write remote memory error");
                        drop(unsafe { CString::from_raw(v as *mut c_char) });
                        Some(remote_addr)
                    }
                } else {
                    None
                }
            }
        }
    };
}

fn alloc_remote_mem(
    remote: &mut Tracee,
    remote_mem: Rc<RefCell<Option<RemoteMem>>>,
    size: usize,
) -> usize {
    let mut mem = remote_mem.borrow_mut();
    if mem.is_none() {
        *mem = Some(RemoteMem::new(remote.pid.as_raw()));
    }

    let mut mem = mem.as_mut().unwrap();
    if size > mem.max {
        panic!("changed content is too large");
    }

    let addr = mem.base + mem.offset;
    if mem.offset + size > mem.max {
        mem.offset = 0;
    } else {
        mem.offset += size;
    }

    addr
}

pub trait Write<T> {
    fn write(
        &mut self,
        remote: &mut Tracee,
        remote_mem: Rc<RefCell<Option<RemoteMem>>>,
        v: Option<T>,
    ) -> Option<u64>;
}

macro_rules! not_ptr_impl {
    ($t: ty) => {
        impl Read for $t {
            type InnerType = $t;

            fn read(_: &mut Tracee, u: u64) -> MayBePtr<Self::InnerType> {
                MayBePtr {
                    inner: u as $t,
                    origin: u,
                }
            }
        }

        impl Write<$t> for MayBePtr<$t> {
            fn write(
                &mut self,
                _remote: &mut Tracee,
                _remote_mem: Rc<RefCell<Option<RemoteMem>>>,
                v: Option<$t>,
            ) -> Option<u64> {
                v.map(|x| x as u64)
            }
        }

        impl Ptr<$t> for MayBePtr<$t> {
            fn get(&self) -> $t {
                self.inner
            }
        }

        impl Number for $t {
            fn from_u64(u: u64) -> Self {
                u as Self
            }

            fn to_u64(self) -> u64 {
                self as u64
            }
        }
    };
}

ptr_impl!(*const c_char);
ptr_impl!(*mut c_char);
not_ptr_impl!(i8);
not_ptr_impl!(i16);
not_ptr_impl!(i32);
not_ptr_impl!(i64);
not_ptr_impl!(isize);
not_ptr_impl!(u8);
not_ptr_impl!(u16);
not_ptr_impl!(u32);
not_ptr_impl!(u64);
not_ptr_impl!(usize);

pub trait Ptr<T> {
    fn get(&self) -> T;
}

pub struct MayBePtr<T> {
    inner: T,
    origin: u64,
}
