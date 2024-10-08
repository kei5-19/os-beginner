#![no_std]
#![cfg(target_arch = "x86_64")]

#[cfg(feature = "alloc")]
mod _alloc;
mod syscall;

pub mod args;
pub mod buf;
pub mod errno;
pub mod events;
pub mod fs;
pub mod graphics;
pub mod io;
pub mod logger;
pub mod stdio;
pub mod time;
pub mod unistd;

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
pub use crate::_alloc::Global;

pub use app_lib_macros::main;

pub use errno::ERRNO;

pub fn exit(exit_code: i32) -> ! {
    unsafe { syscall::__exit(exit_code as _) };
    core::unreachable!("syscall exit never returns")
}
