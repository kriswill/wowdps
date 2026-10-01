//! Bake the dev shell's dlopen search path into the binary.
//!
//! winit reaches for libwayland-client, wgpu for libvulkan / libGL — all via
//! dlopen, so nothing links them and the linker records no runpath for them.
//! On NixOS they live only under /nix/store, which the flake dev shell exposes
//! through `LD_LIBRARY_PATH`; a binary run from anywhere else (the daemon's
//! overlay supervisor, a plain terminal) would panic with `NoWaylandLib`.
//! glibc's dlopen consults the calling object's RUNPATH, and the caller is
//! this executable, so recording those directories here makes the built
//! binary self-sufficient. Outside such a shell the variable is unset and
//! this emits nothing.
fn main() {
    println!("cargo:rerun-if-env-changed=LD_LIBRARY_PATH");
    if !cfg!(target_os = "linux") {
        return;
    }
    let Ok(path) = std::env::var("LD_LIBRARY_PATH") else {
        return;
    };
    for dir in path.split(':').filter(|d| !d.is_empty()) {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,{dir}");
    }
}
