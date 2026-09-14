//! Link-time compatibility shim: provides `__rust_probestack` for wasmer on Linux x86_64.
//!
//! wasmer-vm 5.x declares `extern "C" { fn __rust_probestack(); }` on non-Windows
//! x86/x86_64 (`wasmer_vm::probestack::PROBESTACK`, exposed as the `LibCall::Probestack`
//! libcall that Cranelift-emitted wasm code calls to probe large stack frames).
//! That symbol was historically defined in the Rust runtime's compiler_builtins,
//! but modern rustc no longer defines it (the definition is now gated behind the
//! `unmangled-names` cargo feature, which rustc's sysroot build does not enable),
//! so any Linux link against wasmer 5 fails with
//! `undefined symbol: __rust_probestack`.
//!
//! This module supplies the verbatim implementation from
//! `rust-lang/compiler-builtins` (MIT/Apache-2.0), preserving the stack-clash
//! guard: Cranelift passes the frame size in `rax` and the probe touches each
//! page from `rsp` down so an overflow is guaranteed to hit the guard page.
//! It must preserve all registers except `rax` and the flags, hence `#[naked]`.
//!
//! Only x86_64 is provided (the only Linux target we build). Other non-Windows
//! x86 targets linking wasmer 5 would still need the 32-bit variant.

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[no_mangle]
#[unsafe(naked)]
pub unsafe extern "C" fn __rust_probestack() {
    core::arch::naked_asm!(
        "
            .cfi_startproc
            push  rbp
            .cfi_adjust_cfa_offset 8
            .cfi_offset rbp, -16
            mov   rbp, rsp
            .cfi_def_cfa_register rbp

            mov    r11, rax        // duplicate rax as we're clobbering r11

            // Main loop, taken in one page increments. We're decrementing rsp by
            // a page each time until there's less than a page remaining. We're
            // guaranteed that this function isn't called unless there's more than a
            // page needed.
            //
            // Note that we're also testing against `[rsp + 8]` to account for the 8
            // bytes pushed on the stack originally with our return address. Using
            // `[rsp + 8]` simulates us testing the stack pointer in the caller's
            // context.

            // It's usually called when rax >= 0x1000, but that's not always true.
            // Dynamic stack allocation, which is needed to implement unsized
            // rvalues, triggers stackprobe even if rax < 0x1000.
            // Thus we have to check r11 first to avoid segfault.
            cmp    r11, 0x1000
            jna    3f
        2:
            sub    rsp, 0x1000
            test   qword ptr [rsp + 8], rsp
            sub    r11, 0x1000
            cmp    r11, 0x1000
            ja     2b

        3:
            // Finish up the last remaining stack space requested, getting the last
            // bits out of r11
            sub    rsp, r11
            test   qword ptr [rsp + 8], rsp

            // Restore the stack pointer to what it previously was when entering
            // this function. The caller will readjust the stack pointer after we
            // return.
            add    rsp, rax

            leave
            .cfi_def_cfa_register rsp
            .cfi_adjust_cfa_offset -8
            ret
            .cfi_endproc
        ",
    );
}
