use btleplug::{api::Manager as _, platform::Manager};
use pluglib::SmartPlug;

#[tokio::main]
async fn main() {
    // Init BLE central
    let manager = Manager::new().await.unwrap();
    let central = manager
        .adapters()
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();

    // Connect to the first found device
    let mut device = pluglib::scan_and_connect(&central, |_| { true }).await.unwrap();
    println!("Connected to {:?}", device);

    device.toggle().await.unwrap();
}