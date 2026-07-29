/* RP2040 memory layout for the `embassy-rp` stack.
 *
 * Unlike the raw rp2040-hal layout there is no `SECTIONS` block here: embassy-rp
 * supplies both the boot2 blob and the `.boot2` placement, via the `link-rp.x`
 * script its build script emits (build.rs passes `-Tlink-rp.x` after `-Tlink.x`).
 * This file only has to declare the regions that script refers to.
 */
MEMORY {
    BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100
    RAM   : ORIGIN = 0x20000000, LENGTH = 264K
}
