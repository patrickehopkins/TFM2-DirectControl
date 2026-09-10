//! Transparent probe for the dominant back-edge inside the confirmed v0.5.8 match runner.
//!
//! Static disassembly of the confirmed client simulation runner found one very large loop:
//!
//!     0x1418175C8 -> 0x1418147F4
//!
//! The loop encloses most of the runner body, making its head the strongest current candidate
//! for the one-step-per-tick boundary. This probe patches only the loop head and increments an
//! atomic counter using a tiny generated machine-code stub. It does not call Rust from inside
//! the runner, does not modify simulation state, and preserves RFLAGS and RAX before jumping
//! through a trampoline containing the displaced original instructions.

use std::{
    ffi::c_void,
    ptr,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        OnceLock,
    },
};

const EXPECTED_PE_TIMESTAMP: u32 = 0x6A97_8218;
const EXPECTED_IMAGE_SIZE: u32 = 0x04A1_D000;

pub const LOOP_HEAD_RVA: usize = 0x0181_47F4;
const PATCH_LEN: usize = 16;
const EXPECTED_BYTES: [u8; PATCH_LEN] = [
    0x41, 0xC6, 0x85, 0x89, 0x20, 0x00, 0x00, 0x00, // mov byte ptr [r13+2089h],0
    0x41, 0x80, 0xBD, 0x88, 0x20, 0x00, 0x00, 0x00, // cmp byte ptr [r13+2088h],0
];

const ABS_INDIRECT_JUMP_LEN: usize = 14;
const TRAMPOLINE_LEN: usize = PATCH_LEN + ABS_INDIRECT_JUMP_LEN;
const STUB_CAPACITY: usize = 64;

const MEM_COMMIT: u32 = 0x1000;
const MEM_RESERVE: u32 = 0x2000;
const PAGE_EXECUTE_READWRITE: u32 = 0x40;

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(module_name: *const u16) -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
    fn VirtualAlloc(
        address: *mut c_void,
        size: usize,
        allocation_type: u32,
        protect: u32,
    ) -> *mut c_void;
    fn VirtualProtect(
        address: *mut c_void,
        size: usize,
        new_protect: u32,
        old_protect: *mut u32,
    ) -> i32;
    fn FlushInstructionCache(process: *mut c_void, address: *const c_void, size: usize) -> i32;
}

static LOOP_ENTRIES: AtomicU64 = AtomicU64::new(0);
static LAST_R13: AtomicUsize = AtomicUsize::new(0);
static INSTALL_RESULT: OnceLock<Result<(), String>> = OnceLock::new();

#[derive(Debug, Clone, Copy)]
pub struct LoopProbeSnapshot {
    pub entries: u64,
    pub last_runner_state: usize,
}

pub fn snapshot() -> LoopProbeSnapshot {
    LoopProbeSnapshot {
        entries: LOOP_ENTRIES.load(Ordering::Acquire),
        last_runner_state: LAST_R13.load(Ordering::Acquire),
    }
}

pub fn ensure_installed() -> Result<(), String> {
    INSTALL_RESULT
        .get_or_init(|| unsafe { install_inner() })
        .clone()
}

unsafe fn install_inner() -> Result<(), String> {
    let module_base = GetModuleHandleW(ptr::null());
    if module_base.is_null() {
        return Err("GetModuleHandleW(NULL) failed".to_owned());
    }

    let base = module_base.cast::<u8>();
    let pe_offset = ptr::read_unaligned(base.add(0x3c).cast::<u32>()) as usize;
    if ptr::read_unaligned(base.add(pe_offset).cast::<u32>()) != 0x0000_4550 {
        return Err("main module does not contain a valid PE signature".to_owned());
    }

    let timestamp = ptr::read_unaligned(base.add(pe_offset + 8).cast::<u32>());
    let optional_header = pe_offset + 24;
    let image_size = ptr::read_unaligned(base.add(optional_header + 56).cast::<u32>());
    if timestamp != EXPECTED_PE_TIMESTAMP || image_size != EXPECTED_IMAGE_SIZE {
        return Err(format!(
            "unsupported TeamfightManager2.exe build (timestamp=0x{timestamp:08X}, image=0x{image_size:08X})"
        ));
    }

    let target = base.add(LOOP_HEAD_RVA);
    let actual = std::slice::from_raw_parts(target, PATCH_LEN);
    if actual != EXPECTED_BYTES {
        return Err(format!(
            "runner loop-head signature mismatch at RVA 0x{LOOP_HEAD_RVA:X}"
        ));
    }

    let trampoline = VirtualAlloc(
        ptr::null_mut(),
        TRAMPOLINE_LEN,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_EXECUTE_READWRITE,
    )
    .cast::<u8>();
    if trampoline.is_null() {
        return Err("VirtualAlloc failed while creating loop trampoline".to_owned());
    }

    ptr::copy_nonoverlapping(target, trampoline, PATCH_LEN);
    write_indirect_abs_jump(trampoline.add(PATCH_LEN), target.add(PATCH_LEN) as usize);

    // Generated transparent stub:
    //   pushfq
    //   push rax
    //   mov rax, &LOOP_ENTRIES
    //   lock inc qword ptr [rax]
    //   mov rax, &LAST_R13
    //   mov [rax], r13
    //   pop rax
    //   popfq
    //   jmp qword ptr [rip+0] ; trampoline
    let stub = VirtualAlloc(
        ptr::null_mut(),
        STUB_CAPACITY,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_EXECUTE_READWRITE,
    )
    .cast::<u8>();
    if stub.is_null() {
        return Err("VirtualAlloc failed while creating loop counter stub".to_owned());
    }

    let counter_address = (&LOOP_ENTRIES as *const AtomicU64) as usize;
    let state_address = (&LAST_R13 as *const AtomicUsize) as usize;
    let mut code = Vec::with_capacity(STUB_CAPACITY);
    code.push(0x9C); // pushfq
    code.push(0x50); // push rax
    code.extend_from_slice(&[0x48, 0xB8]); // mov rax, imm64
    code.extend_from_slice(&counter_address.to_le_bytes());
    code.extend_from_slice(&[0xF0, 0x48, 0xFF, 0x00]); // lock inc qword ptr [rax]
    code.extend_from_slice(&[0x48, 0xB8]); // mov rax, imm64
    code.extend_from_slice(&state_address.to_le_bytes());
    code.extend_from_slice(&[0x4C, 0x89, 0x28]); // mov [rax], r13
    code.push(0x58); // pop rax
    code.push(0x9D); // popfq
    code.extend_from_slice(&[0xFF, 0x25, 0x00, 0x00, 0x00, 0x00]); // jmp [rip+0]
    code.extend_from_slice(&(trampoline as usize).to_le_bytes());

    if code.len() > STUB_CAPACITY {
        return Err("internal loop stub exceeded allocation size".to_owned());
    }
    ptr::copy_nonoverlapping(code.as_ptr(), stub, code.len());

    let mut old_protect = 0u32;
    if VirtualProtect(
        target.cast::<c_void>(),
        PATCH_LEN,
        PAGE_EXECUTE_READWRITE,
        &mut old_protect,
    ) == 0
    {
        return Err("VirtualProtect failed while enabling loop-head detour write".to_owned());
    }

    write_indirect_abs_jump(target, stub as usize);
    for offset in ABS_INDIRECT_JUMP_LEN..PATCH_LEN {
        *target.add(offset) = 0x90;
    }

    let process = GetCurrentProcess();
    let mut ignored = 0u32;
    let _ = VirtualProtect(target.cast::<c_void>(), PATCH_LEN, old_protect, &mut ignored);
    let _ = FlushInstructionCache(process, trampoline.cast::<c_void>(), TRAMPOLINE_LEN);
    let _ = FlushInstructionCache(process, stub.cast::<c_void>(), code.len());
    let _ = FlushInstructionCache(process, target.cast::<c_void>(), PATCH_LEN);

    Ok(())
}

unsafe fn write_indirect_abs_jump(destination: *mut u8, target: usize) {
    // jmp qword ptr [rip+0] ; <absolute 64-bit target>
    *destination = 0xFF;
    *destination.add(1) = 0x25;
    *destination.add(2) = 0x00;
    *destination.add(3) = 0x00;
    *destination.add(4) = 0x00;
    *destination.add(5) = 0x00;
    ptr::copy_nonoverlapping(
        target.to_le_bytes().as_ptr(),
        destination.add(6),
        std::mem::size_of::<usize>(),
    );
}
