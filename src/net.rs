use core::net::Ipv4Addr;

use embassy_executor::Spawner;
use embassy_net::{Runner, StackResources, tcp::TcpSocket};
use embassy_time::{Duration, Timer};
use esp_alloc as _;

use embassy_sync::pipe::{Pipe, Reader, Writer};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, blocking_mutex::raw::NoopRawMutex, signal::Signal,
};
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
use espeos::MsgType;

const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");

#[embassy_executor::task]
pub async fn connection(
    mut controller: WifiController<'static>,
    cli_pipe: &'static Reader<'static, CriticalSectionRawMutex, 256>,
) {
    println!("start connection task");
    println!("Device capabilities: {:?}", controller.capabilities());

    //temporary, eventually want to write to flash in the CLI task or this task
    //
    let mut buf = [0u8; 256];
    let mut ssid = false;
    let mut pass = false;
    loop {
        let read_size = cli_pipe.read(&mut buf).await;
        if read_size == 0 {
            println! {"dead pipe {} {:?}",read_size, buf};
            //Timer::after_secs(100).await;
        }
        //let slice = &buf[0..read_size];
        let msgtype: MsgType = buf[0].into();
        let msgbody = core::str::from_utf8(&buf[1..read_size]).unwrap();

        //todo recieve msgtype enum to tell what message is
        println! {"in connection task {:?} {:?}", msgtype, msgbody};

        if ssid && pass {
            break;
        }

        Timer::after(Duration::from_millis(500)).await;
    }

    loop {
        match esp_radio::wifi::sta_state() {
            WifiStaState::Connected => {
                // wait until we're no longer connected
                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                Timer::after(Duration::from_millis(5000)).await
            }
            _ => {}
        }
        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = ModeConfig::Client(
                ClientConfig::default()
                    .with_ssid(SSID.into())
                    .with_password(PASSWORD.into()),
            );
            controller.set_config(&client_config).unwrap();
            println!("Starting wifi");
            controller.start_async().await.unwrap();
            println!("Wifi started!");

            println!("Scan");
            let scan_config = ScanConfig::default().with_max(10);
            let result = controller
                .scan_with_config_async(scan_config)
                .await
                .unwrap();
            for ap in result {
                println!("{:?}", ap);
            }
        }
        println!("About to connect...");

        match controller.connect_async().await {
            Ok(_) => println!("Wifi connected!"),
            Err(e) => {
                println!("Failed to connect to wifi: {e:?}");
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}

#[embassy_executor::task]
pub async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}
