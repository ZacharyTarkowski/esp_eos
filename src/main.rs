#![no_std]
#![no_main]

use core::net::Ipv4Addr;

use embassy_executor::Spawner;
use embassy_net::{StackResources, tcp::TcpSocket};
use embassy_time::{Duration, Timer};
use esp_alloc as _;

use embassy_sync::{
    blocking_mutex::{CriticalSectionMutex, raw::NoopRawMutex},
    pipe::{Pipe, Reader, Writer},
};

#[cfg(target_arch = "riscv32")]
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::{clock::CpuClock, ram, rng::Rng, timer::timg::TimerGroup};
use esp_println::println;
use esp_radio::Controller;

use esp_hal::i2c::master::I2c;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use esp_hal::uart::{AtCmdConfig, Config, RxConfig, Uart};
use static_cell::StaticCell;

use embedded_graphics::{
    mono_font::{MonoTextStyleBuilder, ascii::FONT_6X13},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Baseline, Text},
};

use ssd1306::{I2CDisplayInterface, Ssd1306, Ssd1306Async, prelude::*};

//todo need to put in env
// fifo_full_threshold (RX)
const READ_BUF_SIZE: usize = 64;
// EOT (CTRL-D)
const AT_CMD: u8 = 0x04;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    println!("Panic!");
    loop {}
}

esp_bootloader_esp_idf::esp_app_desc!();

// When you are okay with using a nightly compiler it's better to use https://docs.rs/static_cell/2.1.0/static_cell/macro.make_static.html
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

mod alarm;
mod net;
mod usb;

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    esp_println::logger::init_logger_from_env();
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[ram(reclaimed)] size: 64 * 1024);
    esp_alloc::heap_allocator!(size: 36 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    #[cfg(target_arch = "riscv32")]
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(
        timg0.timer0,
        #[cfg(target_arch = "riscv32")]
        sw_int.software_interrupt0,
    );

    let esp_radio_ctrl = &*mk_static!(Controller<'static>, esp_radio::init().unwrap());

    let (controller, interfaces) =
        esp_radio::wifi::new(&esp_radio_ctrl, peripherals.WIFI, Default::default()).unwrap();

    let wifi_interface = interfaces.sta;

    let config = embassy_net::Config::dhcpv4(Default::default());

    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    // Init network stack
    let (stack, runner) = embassy_net::new(
        wifi_interface,
        config,
        mk_static!(StackResources<3>, StackResources::<3>::new()),
        seed,
    );

    let sck = peripherals.GPIO2;
    let sda = peripherals.GPIO3;
    let config =
        esp_hal::i2c::master::Config::default().with_frequency(esp_hal::time::Rate::from_khz(400));
    let mut i2c = I2c::new(peripherals.I2C0, config)
        .unwrap()
        .with_sda(sda)
        .with_scl(sck)
        .into_async();

    let interface = I2CDisplayInterface::new(i2c);
    let mut display = Ssd1306Async::new(interface, DisplaySize128x32, DisplayRotation::Rotate0)
        .into_terminal_mode();
    display.init().await.unwrap();
    display.clear().await.unwrap();

    let _ = display.write_str("Eos Boot").await;

    let config = esp_hal::gpio::OutputConfig::default();
    let alarm_pin: esp_hal::gpio::Output<'_> =
        esp_hal::gpio::Output::new(peripherals.GPIO10, esp_hal::gpio::Level::High, config);

    println!("mac is {:x?}", esp_radio::wifi::sta_mac());

    // Default pins for Uart communication
    let (tx_pin, rx_pin) = (peripherals.GPIO21, peripherals.GPIO20);

    let config = Config::default()
        .with_rx(RxConfig::default().with_fifo_full_threshold(READ_BUF_SIZE as u16));

    let mut uart0 = Uart::new(peripherals.UART0, config)
        .unwrap()
        .with_tx(tx_pin)
        .with_rx(rx_pin)
        .into_async();
    uart0.set_at_cmd(AtCmdConfig::default().with_cmd_char(AT_CMD));

    let (rx, _) = uart0.split();

    static CLI_PIPE: StaticCell<Pipe<CriticalSectionRawMutex, 256>> = StaticCell::new();
    let cli_pipe = &mut *CLI_PIPE.init(Pipe::new());
    let (reader, writer) = cli_pipe.split();
    static WRITER: StaticCell<Writer<'static, CriticalSectionRawMutex, 256>> = StaticCell::new();
    let writer = &*WRITER.init(writer);
    static READER: StaticCell<Reader<'static, CriticalSectionRawMutex, 256>> = StaticCell::new();
    let reader = &*READER.init(reader);

    //todo wrap display in a mutex so that CLI commands can print to it.
    //let display_mutex = embassy_sync::mutex::Mutex::new(Ssd1306Async);

    //todo
    // function is obe, cant make multiple static, duh.
    let (connection_pipe_reader, connection_pipe_writer) = make_static_pipe_split();
    spawner.spawn(usb::reader(rx, &writer, display)).ok();

    //Dont need a usb writer, just using debug statements. Eventually output is through display.
    //spawner.spawn(usb::writer(tx)).ok();

    spawner
        .spawn(usb::cli_task(&reader, &connection_pipe_writer))
        .ok();

    spawner
        .spawn(net::connection(controller, connection_pipe_reader))
        .ok();

    spawner.spawn(net::net_task(runner)).ok();

    spawner.spawn(alarm::run_alarm(alarm_pin)).ok();

    println!("spawned all tasks");

    //todo
    // make this a task in net

    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];

    loop {
        if stack.is_link_up() {
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    println!("Waiting to get IP address...");
    loop {
        if let Some(config) = stack.config_v4() {
            println!("Got IP: {}", config.address);
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    loop {
        Timer::after(Duration::from_millis(1_000)).await;

        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);

        socket.set_timeout(Some(embassy_time::Duration::from_secs(10)));

        let remote_endpoint = (Ipv4Addr::new(142, 250, 185, 115), 80);
        println!("connecting...");
        let r = socket.connect(remote_endpoint).await;
        if let Err(e) = r {
            println!("connect error: {:?}", e);
            continue;
        }
        println!("connected!");
        let mut buf = [0; 1024];
        loop {
            //use embedded_io_async::Write;
            let r = socket
                .write(b"GET / HTTP/1.1\r\nHost: worldtimeapi.org/api/ip\r\n\r\n")
                .await;
            if let Err(e) = r {
                println!("write error: {:?}", e);
                break;
            }
            let n = match socket.read(&mut buf).await {
                Ok(0) => {
                    println!("read EOF");
                    break;
                }
                Ok(n) => n,
                Err(e) => {
                    println!("read error: {:?}", e);
                    break;
                }
            };
            println!("{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        Timer::after(Duration::from_millis(3000)).await;
    }
}

fn make_static_pipe() -> &'static mut Pipe<CriticalSectionRawMutex, 256> {
    static PIPE: StaticCell<Pipe<CriticalSectionRawMutex, 256>> = StaticCell::new();
    let pipe = &mut *PIPE.init(Pipe::new());
    pipe
}

fn make_static_pipe_split() -> (
    &'static Reader<'static, CriticalSectionRawMutex, 256>,
    &'static Writer<'static, CriticalSectionRawMutex, 256>,
) {
    let (cli_pipe_reader, cli_pipe_writer) = make_static_pipe().split();

    static READER: StaticCell<Reader<'static, CriticalSectionRawMutex, 256>> = StaticCell::new();
    let reader = &*READER.init(cli_pipe_reader);

    static WRITER: StaticCell<Writer<'static, CriticalSectionRawMutex, 256>> = StaticCell::new();
    let writer = &*WRITER.init(cli_pipe_writer);

    (reader, writer)
}
