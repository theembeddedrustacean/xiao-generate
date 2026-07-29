/* RP2350 memory layout, shared by the `rp235x-hal` and `embassy-rp` stacks.
 *
 * Source: rp-rs/rp235x-project-template `memory.x` (verified: the same script
 * links both the raw rp235x-hal build and the embassy-rp `rp235xa` build).
 *
 * The RP2350 has no 2nd-stage bootloader blob; the boot ROM instead reads an
 * IMAGE_DEF "block" from `.start_block` near the top of flash. rp235x-hal
 * projects declare that block themselves, embassy-rp emits one for you — either
 * way it needs the sections below.
 */
MEMORY {
    /* 2 MiB is the safe default for an RP2350 with external flash. */
    FLASH : ORIGIN = 0x10000000, LENGTH = 2048K
    /* SRAM0-SRAM7, striped mapping. */
    RAM : ORIGIN = 0x20000000, LENGTH = 512K
    /* Banks 8 and 9 are directly mapped (e.g. a dedicated per-core stack). */
    SRAM4 : ORIGIN = 0x20080000, LENGTH = 4K
    SRAM5 : ORIGIN = 0x20081000, LENGTH = 4K
}

SECTIONS {
    /* ### Boot ROM info
     *
     * Goes after .vector_table, to keep it in the first 4K of flash
     * where the Boot ROM (and picotool) can find it
     */
    .start_block : ALIGN(4)
    {
        __start_block_addr = .;
        KEEP(*(.start_block));
        KEEP(*(.boot_info));
    } > FLASH

} INSERT AFTER .vector_table;

/* move .text to start /after/ the boot info */
_stext = ADDR(.start_block) + SIZEOF(.start_block);

SECTIONS {
    /* ### Picotool 'Binary Info' Entries
     *
     * Picotool looks through this block (as we have pointers to it in our
     * header) to find interesting information.
     */
    .bi_entries : ALIGN(4)
    {
        /* We put this in the header */
        __bi_entries_start = .;
        /* Here are the entries */
        KEEP(*(.bi_entries));
        /* Keep this block a nice round size */
        . = ALIGN(4);
        /* We put this in the header */
        __bi_entries_end = .;
    } > FLASH
} INSERT AFTER .text;

SECTIONS {
    /* ### Boot ROM extra info
     *
     * Goes after everything in our program, so it can contain a signature.
     */
    .end_block : ALIGN(4)
    {
        __end_block_addr = .;
        KEEP(*(.end_block));
    } > FLASH

} INSERT AFTER .uninit;

PROVIDE(start_to_end = __end_block_addr - __start_block_addr);
PROVIDE(end_to_start = __start_block_addr - __end_block_addr);
