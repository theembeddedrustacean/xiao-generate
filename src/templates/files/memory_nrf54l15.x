/* nRF54L15 memory layout for the XIAO nRF54L15.
 *
 * Unlike the XIAO nRF52840 (whose stock Adafruit UF2 bootloader + SoftDevice
 * force a 0x27000 application offset), the nRF54L15 has NO resident bootloader on
 * this board: it executes in place from RRAM mapped at 0x00000000 and is flashed
 * with `probe-rs run --chip nrf54l15` over the board's onboard SAMD11 CMSIS-DAP
 * debugger. So the application owns flash from 0x0 and the vector table sits there
 * (cortex-m-rt's `link.x` places `.vector_table` at ORIGIN(FLASH)).
 *
 * FLASH LENGTH is the application RRAM the CPU core is given (1524K). RAM starts
 * at 0x20000000, but only the low 128K is handed to the application: the
 * `256K - 128K` subtraction deliberately RESERVES the upper 128K of the 256K SRAM
 * for the FLPR (Fast Lightweight Peripheral Processor) coprocessor. A build that
 * never uses the FLPR could widen RAM to the full 256K; this conservative split
 * is the upstream-verified value.
 *
 * Values verified against embassy-rs/embassy `examples/nrf54l15-app`.
 */
MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 1524K
  RAM   : ORIGIN = 0x20000000, LENGTH = 256K - 128K
}
