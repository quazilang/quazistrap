/* Example freestanding linker layout, NOT a boot protocol implementation. */
OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)
PHDRS {
    text PT_LOAD FLAGS(5);   /* RX */
    rodata PT_LOAD FLAGS(4); /* R */
    data PT_LOAD FLAGS(6);   /* RW */
}
SECTIONS
{
    . = 0x100000;
    .text : ALIGN(0x1000) { KEEP(*(.text.boot)) *(.text .text.*) } :text
    .rodata : ALIGN(0x1000) { *(.rodata .rodata.*) } :rodata
    .data : ALIGN(0x1000) { *(.data .data.*) } :data
    .bss (NOLOAD) : ALIGN(0x1000) {
        *(COMMON)
        *(.bss .bss.*)
    } :data
    /DISCARD/ : { *(.note.GNU-stack) *(.eh_frame .eh_frame.*) }
}
