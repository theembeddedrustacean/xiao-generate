#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::{SdCard, TimeSource, Timestamp, VolumeIdx, VolumeManager};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::main;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::time::Rate;
use esp_println::println;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

/// Dummy time source for embedded-sdmmc. Real applications should back this with
/// an RTC; a fixed timestamp is a documented, valid `TimeSource` implementation.
struct DummyTimeSource;

impl TimeSource for DummyTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 55, // 2025
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

#[main]
fn main() -> ! {
    // generator version: 1.3.0
    // XIAO ESP32-S3 Sense microSD (SPI): SCK=GPIO7, MISO=GPIO8, MOSI=GPIO9, CS=GPIO21

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    println!("\n=== XIAO ESP32-S3 Sense microSD ===");

    // === SPI bus setup ===
    // Start slow (400 kHz) for SD card initialization; a real app can raise the
    // clock afterwards via `apply_config`.
    let spi_config = SpiConfig::default()
        .with_frequency(Rate::from_khz(400))
        .with_mode(SpiMode::_0);

    let spi_bus = Spi::new(peripherals.SPI2, spi_config)
        .expect("failed to init SPI")
        .with_sck(peripherals.GPIO7)
        .with_miso(peripherals.GPIO8)
        .with_mosi(peripherals.GPIO9);

    // CS = GPIO21 (also drives the onboard LED on the XIAO; used purely as CS here).
    let cs = Output::new(peripherals.GPIO21, Level::High, OutputConfig::default());

    // Wrap the raw SPI bus into a blocking SpiDevice with dedicated CS + delay.
    let spi_device =
        ExclusiveDevice::new(spi_bus, cs, Delay::new()).expect("failed to create SPI device");

    // === SD card + FAT volume ===
    let sdcard = SdCard::new(spi_device, Delay::new());

    match sdcard.num_bytes() {
        Ok(size) => println!("SD card detected: {} bytes", size),
        Err(e) => {
            println!("SD card init failed: {:?}", e);
            loop {}
        }
    }

    let volume_mgr = VolumeManager::new(sdcard, DummyTimeSource);

    let volume0 = volume_mgr
        .open_volume(VolumeIdx(0))
        .expect("failed to open volume 0");
    println!("Volume 0 opened");

    let root_dir = volume0.open_root_dir().expect("failed to open root dir");
    println!("Root directory entries:");

    root_dir
        .iterate_dir(|entry| {
            println!("  {} ({} bytes)", entry.name, entry.size);
        })
        .expect("failed to iterate root dir");

    println!("Done.");

    loop {}
}
