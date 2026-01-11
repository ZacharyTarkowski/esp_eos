use embassy_net::Runner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pipe::Reader;
use embassy_time::{Duration, Timer};
#[cfg(target_arch = "riscv32")]
use esp_println::println;
use esp_radio::wifi::{
    ClientConfig, ModeConfig, ScanConfig, WifiController, WifiDevice, WifiEvent, WifiStaState,
};
use espeos::MsgType;

#[embassy_executor::task]
pub async fn connection(
    mut controller: WifiController<'static>,
    cli_pipe: &'static Reader<'static, CriticalSectionRawMutex, 256>,
) {
    println!("start connection task");
    println!("Device capabilities: {:?}", controller.capabilities());

    let mut ssid = [0u8; 256];
    let mut pass = [0u8; 256];

    let mut ssid_str = core::str::from_utf8(&ssid).unwrap();
    let mut pass_str = core::str::from_utf8(&pass).unwrap();

    //temporary, eventually want to write to flash in the CLI task or this task
    //
    let mut buf = [0u8; 256];
    let mut ssid_recv = false;
    let mut pass_recv = false;
    loop {
        let read_size = cli_pipe.read(&mut buf).await;
        if read_size == 0 {
            println! {"dead pipe {} {:?}",read_size, buf};
            //Timer::after_secs(100).await;
        }
        //let slice = &buf[0..read_size];
        let msgtype: MsgType = buf[0].into();
        let msgbody_slice = &buf[1..read_size];
        let msgbody = core::str::from_utf8(&msgbody_slice).expect("Msgbody bad slice?");

        //todo recieve msgtype enum to tell what message is
        println!("in connection task {:?} {:?}", msgtype, msgbody);
        println!("here");
        match msgtype {
            MsgType::WifiSSID => {
                println!(
                    "here2 {} {} {}",
                    msgbody_slice.len(),
                    ssid[0..read_size - 1].len(),
                    ssid[0..read_size].len()
                );
                ssid[0..read_size - 1].copy_from_slice(&msgbody_slice);
                println!("here3");
                ssid_str = core::str::from_utf8(&ssid[0..read_size]).expect("Bad SSID String");
                ssid_recv = true;
            }
            MsgType::WifiPass => {
                pass[0..read_size - 1].copy_from_slice(msgbody_slice);
                pass_str = core::str::from_utf8(&pass[0..read_size]).expect("Bad Pass String");
                pass_recv = true;
            }
            _ => (),
        }
        println!("here4");
        if ssid_recv && pass_recv {
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
                    .with_ssid(ssid_str.into())
                    .with_password(pass_str.into()),
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
