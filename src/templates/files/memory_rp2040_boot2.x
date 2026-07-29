/* RP2040 memory layout for the raw `rp2040-hal` stack.
 *
 * Source: rp-rs/rp-hal `rp2040-hal-examples/memory.x`
 * (provenance 49daec26ed28977a6863ebb510d7cd2f41c49316).
 *
 * The RP2040 boot ROM can only run the first 256 bytes of flash, so the program
 * carries its own 2nd-stage bootloader (the `BOOT2` static in main.rs, from the
 * `rp2040-boot2` crate) and this script places it at the start of flash.
 */
MEMORY {
    BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100
    RAM : ORIGIN = 0x20000000, LENGTH = 256K
    SRAM4 : ORIGIN = 0x20040000, LENGTH = 4k
    SRAM5 : ORIGIN = 0x20041000, LENGTH = 4k
}

EXTERN(BOOT2_FIRMWARE)

SECTIONS {
    .boot2 ORIGIN(BOOT2) :
    {
        KEEP(*(.boot2));
    } > BOOT2
} INSERT BEFORE .text;
