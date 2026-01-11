use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_println as _;

#[embassy_executor::task]
pub async fn run_alarm(mut pin: esp_hal::gpio::Output<'static>) {
    let mut tone: bool = false;
    let mut pulse: bool = false;
    let tone_freq_hz = 2;
    let pulse_freq_hz = 500;
    let tone_t_ms = 1000 / tone_freq_hz;
    let pulse_t_ms = 1000 / pulse_freq_hz;

    let num_pulses_per_tone = pulse_freq_hz / tone_freq_hz;

    loop {
        if tone == true {
            for _ in 0..num_pulses_per_tone {
                match pulse {
                    true => pin.set_high(),
                    false => pin.set_low(),
                }
                pulse = !pulse;
                Timer::after(Duration::from_millis(pulse_t_ms)).await;
            }
            tone = false;
        } else {
            pin.set_low();
            Timer::after(Duration::from_millis(tone_t_ms)).await;
            tone = true;
        }
    }
}
