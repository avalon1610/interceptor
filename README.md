# interceptor

Interceptor is a lib based on `ptrace` that intercepts and modifies Linux system calls.
It currently only supports `x86_64` architecture.

## Usage
Write a function whose signature is same as a syscall, and mark it as `#[syscall]`,
and you are done.

```rust
#[syscall]
fn openat(dfd: i32, mut filename: *const c_char, flags: i32, mode: i32) -> i32 {
    // do something before syscall, logging, changing arguments, etc.

    let ret = real!(dfd, filename, flags, mode);

    // do something after syscall, modifing return value..
}
```
See more detail in examples

## Extra Info

### Memory in target
We use "LD_PRELOAD" trick to insert a so into target process to malloc extra memory
needed when modified a pointer argument which has larger length.

### Remove dependency libgcc_s.so.1
Some glibc released without `libgcc_s.so.1`, we removed this dependency using link
script "linker_without_libgcc.wrap".

### special handling of parameter "*const *const c_char"

> This parameter is used in syscall like `execve`'s `const char *const argv[]`.

We treat `*const *const c_char` as `*const c_char` for convenience. That's say, original
pointer to pointer has been converted to pointer's content after content. For example:

##### original
this ptr's address is inside target process, can not be used directly.

| ptr (*const *const c_char)    | ptr to ptr (*const c_char)    | content   |
| ---                           | ---                           | ---       |
| 0x12345678                    | 0x11111111                    | "aaaa\0"  |
|                               | 0x22222222                    | "bbbb\0"  |
|                               | 0x33333333                    | "cccc\0"  |
|                               | 0x44444444                    | "\0"      |
#### after converted
memory is reallocated in interceptor instance, so address changed.

| ptr (*const c_char)   | content                   |
| ---                   | ---                       |
| 0x87654321            | "aaaa\0bbbb\0cccc\0\0"    |
#### usage
you can use helper function [`read_ptr_to_ptr`] to read content from converted ptr.
and use [`write_ptr_to_ptr`] to write back.

