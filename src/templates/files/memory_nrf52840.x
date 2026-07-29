/* nRF52840 memory layout for a XIAO nRF52840 / XIAO nRF52840 Sense running the
 * STOCK bootloader Seeed ships on the board: the Adafruit nRF52 UF2 bootloader
 * with SoftDevice S140 7.3.0 (Adafruit_nRF52_Bootloader
 * src/boards/xiao_nrf52840_ble{,_sense}/board.mk — `MCU_SUB_VARIANT = nrf52840`,
 * `SD_VERSION = 7.3.0`). Shared by the `nrf52840-hal` and `embassy-nrf` stacks;
 * neither HAL bundles a memory.x, so the project ships one.
 *
 * THIS IS NOT A FLAT BARE-METAL LAYOUT AND MUST NOT BECOME ONE. On a stock
 * board the 1 MiB of flash is already occupied at BOTH ends:
 *
 *   0x00000000  MBR + SoftDevice S140 7.3.0  156K  resident, must not be written
 *   0x00027000  application                  780K  <- this script
 *   0x000EA000  app-data reserve              40K  UF2 writes here are refused
 *   0x000F4000  Adafruit UF2 bootloader       48K  resident, must not be written
 *                  0xF4000 bootloader code
 *                  0xFE000 MBR parameter page
 *                  0xFF000 bootloader settings page
 *
 * Link the application at 0x00000000, copy the resulting UF2 to the board's
 * mass-storage drive, and it lands on top of the MBR and the SoftDevice.
 * Recovering from that needs an SWD probe. Hence FLASH at 0x27000.
 *
 * 0x27000 IS EXACT, NOT A FLOOR. The bootloader jumps to whatever
 * `SD_SIZE_GET(MBR_SIZE)` reports — the size the SoftDevice image records about
 * itself (Adafruit_nRF52_Bootloader
 * lib/sdk11/components/libraries/bootloader_dfu/bootloader.c: `app_addr =
 * SD_SIZE_GET(MBR_SIZE)`), which for S140 7.3.0 is 0x27000. An application
 * linked anywhere else is not bricking anything, it simply never starts.
 * (S140 v6 answers 0x26000 — that is why older Adafruit linker scripts say
 * 0x26000. It is the wrong number for this board.)
 *
 * LENGTH ends at 0xEA000, not at the bootloader's 0xF4000, because 0xEA000 is
 * the limit the flashing path actually enforces: `USER_FLASH_END =
 * BOOTLOADER_REGION_START - DFU_APP_DATA_RESERVED` (src/usb/uf2/uf2cfg.h) with
 * `BOOTLOADER_REGION_START = 0x000F4000` (lib/sdk11/.../dfu_types.h) and
 * `DFU_APP_DATA_RESERVED = 10*4096` for nRF52840 (Makefile), and
 * src/usb/uf2/ghostfat.c refuses every UF2 block outside `USER_FLASH_START <=
 * addr < USER_FLASH_END`. An image that overran 780K would not be rejected as a
 * whole — it would be written with its tail quietly dropped. Failing at link
 * time instead is the safe direction to be wrong in. (Zephyr's `xiao_ble`
 * partition map is slightly more generous at 788K, ending 0xEC000; it predates
 * nothing in particular, it just does not model the bootloader's app-data
 * reserve. 780K is the intersection of the two.)
 *
 * RAM STARTS 8 BYTES LATE ON PURPOSE, EVEN THOUGH NOTHING HERE ENABLES THE
 * SOFTDEVICE. The bootloader hands control over with the procedure it documents
 * in src/main.c — initialise the MBR, disable the SoftDevice, call
 * `sd_softdevice_vector_table_base_set(APP_ADDR)`, jump — so the MBR at 0x0, not
 * the application, owns the vector table, and it forwards every interrupt
 * through a pointer it keeps at the bottom of RAM (bootloader.c writes it:
 * `*(uint32_t *)(0x20000000) = app_addr`). An application that allocates over
 * that pointer faults on its first interrupt. Nordic's SoftDevice Specification
 * §15.1 puts the reservation at 8 bytes and the minimum APP_RAM_BASE at
 * 0x20000008 for the *disabled* SoftDevice; a widely-cited MBR teardown says 4.
 * 8 costs nothing and keeps the 8-byte alignment.
 *
 * Related, same mechanism: because the MBR forwards interrupts rather than the
 * core reading the application's vector table directly, `cortex-m-rt` never
 * needs to write VTOR here — and it does not, unless its `set-vtor` feature is
 * turned on.
 *
 * A build that actually ENABLES a SoftDevice must additionally raise RAM ORIGIN
 * by the RAM that SoftDevice reserves (it reports the real figure back from
 * `sd_ble_enable`). FLASH below is already correct for S140 7.3.0.
 *
 * Flashing, for the record: family ID 0xADA52840, and if you convert from a raw
 * `.bin` the base address is 0x27000 — uf2conv.py's default of 0x2000 is wrong
 * for this board. Converting from the ELF or hex avoids the question.
 */
MEMORY
{
  FLASH : ORIGIN = 0x00027000, LENGTH = 780K
  RAM : ORIGIN = 0x20000008, LENGTH = 256K - 8
}
