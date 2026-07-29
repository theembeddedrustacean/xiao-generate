/* nRF52840 memory layout, shared by the `nrf52840-hal` and `embassy-nrf` stacks.
 *
 * Flat layout: the whole 1 MiB of flash and 256 KiB of RAM belong to the
 * application. Neither HAL bundles a memory.x, so the project ships one.
 *
 * NOTE: this is the *bare-metal* layout. A build that runs under a Nordic
 * SoftDevice (BLE) must move ORIGIN/LENGTH to sit above the SoftDevice — the
 * XIAO nRF52840's stock UF2 bootloader does not require that, but flashing a
 * SoftDevice image does.
 */
MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 1024K
  RAM : ORIGIN = 0x20000000, LENGTH = 256K
}
