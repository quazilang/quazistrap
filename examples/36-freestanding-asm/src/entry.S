/* The Quazi build invokes the C compiler driver in assembler-with-cpp mode.
 * Entry contract: long mode, CPL0, loaded at the linker script address.
 * Customize this file for your chosen loader/boot protocol.
 */
.section .text.boot, "ax", @progbits
.global _start
.type _start, @function
_start:
    lea stack_top(%rip), %rsp
    andq $-16, %rsp
    xor %rbp, %rbp
    call main               /* ordinary Quazi function */
    movl %eax, quazi_result(%rip)
1:
    cli
    hlt
    jmp 1b
.size _start, . - _start

.section .bss.stack, "aw", @nobits
.balign 16
stack_bottom:
    .skip 16384
stack_top:

.section .bss, "aw", @nobits
.global quazi_result
.balign 4
quazi_result:
    .skip 4

.section .note.GNU-stack, "", @progbits
