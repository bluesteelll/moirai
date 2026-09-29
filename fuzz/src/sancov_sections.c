/* SanitizerCoverage section bounds for COFF: fallback A of docs/m0/tools.md §4.4, failure 1.
 *
 * With the sanitizer off (`cargo fuzz ... -s none`), LLVM's SanitizerCoverage pass references __start_ and __stop_
 * symbols for its 8-bit counter, guard, bool-flag and PC-table sections and expects a runtime to define them. ELF
 * linkers synthesise such symbols; on x86_64-pc-windows-msvc only the ASan runtime thunk defines them
 * (sanitizer_coverage_win_sections), so a sanitizer-off fuzz target fails to link with LNK2019/LNK1120. This file
 * defines them the way compiler-rt does. The MSVC linker sorts the grouped sections of one name by the text after
 * '$': "A" (the start object below) < "M" (the instrumented data) < "Z" (the stop object). LLVM adds 8 to each start
 * symbol on COFF, so the 8-byte start objects are skipped and never counted; the 1-byte stop objects mark the end.
 * The counter, guard and bool sections are merged into .data and the PC table into .rdata.
 *
 * fuzz/build.rs compiles this file with the `cc` crate for MSVC targets only, and not under a sanitizer (whose
 * runtime defines the same symbols), and links the object into every fuzz target. Host-only code: nothing here
 * enters a checked graph (docs/m0/PLAN.md §2.2, §2.4).
 */
#if !defined(_MSC_VER)
#error "sancov_sections.c is for the MSVC toolchain (x86_64-pc-windows-msvc) only"
#endif

#include <stdint.h>

#pragma section(".SCOV$CA", read, write)
__declspec(allocate(".SCOV$CA")) uint64_t __start___sancov_cntrs = 0;
#pragma section(".SCOV$CZ", read, write)
__declspec(allocate(".SCOV$CZ")) __declspec(align(1)) uint8_t __stop___sancov_cntrs = 0;

#pragma section(".SCOV$GA", read, write)
__declspec(allocate(".SCOV$GA")) uint64_t __start___sancov_guards = 0;
#pragma section(".SCOV$GZ", read, write)
__declspec(allocate(".SCOV$GZ")) __declspec(align(1)) uint8_t __stop___sancov_guards = 0;

#pragma section(".SCOV$BA", read, write)
__declspec(allocate(".SCOV$BA")) uint64_t __start___sancov_bools = 0;
#pragma section(".SCOV$BZ", read, write)
__declspec(allocate(".SCOV$BZ")) __declspec(align(1)) uint8_t __stop___sancov_bools = 0;

#pragma section(".SCOVP$A", read)
__declspec(allocate(".SCOVP$A")) uint64_t __start___sancov_pcs = 0;
#pragma section(".SCOVP$Z", read)
__declspec(allocate(".SCOVP$Z")) __declspec(align(1)) uint8_t __stop___sancov_pcs = 0;

#pragma comment(linker, "/MERGE:.SCOV=.data")
#pragma comment(linker, "/MERGE:.SCOVP=.rdata")
