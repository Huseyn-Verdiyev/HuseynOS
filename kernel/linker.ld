/* Linker script for HuseynOS kernel (x86_64)
 * Based on the official Limine Rust template.
 * Places kernel in higher-half virtual memory.
 */

OUTPUT_FORMAT(elf64-x86-64)
ENTRY(kmain)

PHDRS
{
    text    PT_LOAD;
    rodata  PT_LOAD;
    data    PT_LOAD;
}

SECTIONS
{
    /* Higher-half kernel base address (top 2 GiB, as required by Limine) */
    . = 0xffffffff80000000;

    .text : {
        *(.text .text.*)
    } :text

    . = ALIGN(CONSTANT(MAXPAGESIZE));

    .rodata : {
        *(.rodata .rodata.*)
    } :rodata

    . = ALIGN(CONSTANT(MAXPAGESIZE));

    .data : {
        *(.data .data.*)

        /* Limine request sections — MUST be inside .data with KEEP */
        KEEP(*(.requests_start_marker))
        KEEP(*(.requests))
        KEEP(*(.requests_end_marker))
    } :data

    .bss : {
        *(.bss .bss.*)
        *(COMMON)
    } :data

    /* Discard unnecessary sections */
    /DISCARD/ : {
        *(.eh_frame*)
        *(.note .note.*)
    }
}
