use core::net::Ipv4Addr;

use embassy_executor::Spawner;
use embassy_net::{Runner, StackResources, tcp::TcpSocket};
use embassy_time::{Duration, Timer};
use esp_alloc as _;

#[cfg(target_arch = "riscv32")]
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::{clock::CpuClock, ram, rng::Rng, timer::timg::TimerGroup};
use esp_println::println;
use esp_radio::{
    Controller,
    wifi::{
        ClientConfig, ModeConfig, ScanConfig, WifiController, WifiDevice, WifiEvent, WifiStaState,
    },
};

use embassy_sync::{blocking_mutex::raw::NoopRawMutex, signal::Signal};
use esp_hal::{
    Async,
    uart::{AtCmdConfig, Config, RxConfig, Uart, UartRx, UartTx},
};
use static_cell::StaticCell;

// fifo_full_threshold (RX)
const READ_BUF_SIZE: usize = 64;
// EOT (CTRL-D)
const AT_CMD: u8 = 0x04;

// enum commands {

// }

#[embassy_executor::task]
pub async fn writer(mut tx: UartTx<'static, Async>, signal: &'static Signal<NoopRawMutex, usize>) {
    use core::fmt::Write;
    embedded_io_async::Write::write(
        &mut tx,
        b"Hello async serial. Enter something ended with EOT (CTRL-D).\r\n",
    )
    .await
    .unwrap();
    embedded_io_async::Write::flush(&mut tx).await.unwrap();
    loop {
        let bytes_read = signal.wait().await;
        signal.reset();
        write!(&mut tx, "\r\n-- received {} bytes --\r\n", bytes_read).unwrap();
        embedded_io_async::Write::flush(&mut tx).await.unwrap();
    }
}

#[embassy_executor::task]
pub async fn reader(mut rx: UartRx<'static, Async>, signal: &'static Signal<NoopRawMutex, usize>) {
    const MAX_BUFFER_SIZE: usize = 10 * READ_BUF_SIZE + 16;

    let mut rbuf: [u8; MAX_BUFFER_SIZE] = [0u8; MAX_BUFFER_SIZE];
    let mut offset = 0;
    loop {
        let r = embedded_io_async::Read::read(&mut rx, &mut rbuf[offset..]).await;
        match r {
            Ok(len) => {
                let new_offset = offset + len;

                //not sure how read function works with buffer overflows?
                // if new_offset > MAX_BUFFER_SIZE {
                //     offset = 0;
                //     new_offset = new_offset - MAX_BUFFER_SIZE;
                // }

                for c in &rbuf[offset..(new_offset)] {
                    match *c {
                        0x0D => {
                            esp_println::print!("\r\n");
                            esp_println::println!(
                                "Full input: {}",
                                str::from_utf8(&rbuf[0..offset]).unwrap()
                            );
                            parse_command(&rbuf[0..offset]);
                            offset = 0; /* do command */
                        }
                        0x08 => {
                            if offset > 0 {
                                offset -= 1;
                                //sp_println::print! {""};
                            };
                        }
                        _ => {
                            if offset < MAX_BUFFER_SIZE {
                                offset += 1;
                            }
                        }
                    }
                    //signal.signal(*c as usize);
                    esp_println::print!("{}", *c as char);
                }
                //offset += len;

                //esp_println::println!("Read: {len}, data: {:x?}", &rbuf[..offset]);
                //offset = 0;
                //signal.signal(len);
            }
            Err(e) => esp_println::println!("RX Error: {:?}", e),
        }
    }
}
#[derive(Debug)]
pub enum AlarmError {
    UnknownCmd,
    BadCredentials,
}

#[derive(Debug)]
pub enum AlarmCmd {
    Wifi,
    Unknown,
}

impl From<&[u8]> for AlarmCmd {
    fn from(buf: &[u8]) -> Self {
        match buf {
            b"Wifi" => AlarmCmd::Wifi,
            _ => AlarmCmd::Unknown,
        }
    }
}

fn parse_command(cmd_buf: &[u8]) -> Result<AlarmCmd, AlarmError> {
    let cmd = match cmd_buf.into() {
        AlarmCmd::Unknown => Err(AlarmError::UnknownCmd),
        x => Ok(x),
    };

    println!("command entered : {:?}", cmd);

    cmd
}
