use btleplug::{
    api::{Central as _, CentralEvent, Peripheral as _, ScanFilter},
    platform::{Adapter, Peripheral},
};
use enum_dispatch::enum_dispatch;
use tokio_stream::StreamExt as _;

mod switchbot;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("BLE error: {0}")]
    Ble(#[from] btleplug::Error),
}

#[enum_dispatch]
#[derive(Debug)]
#[non_exhaustive]
pub enum SmartPlugEnum {
    SwitchBotPlugMini(switchbot::plug_mini::PlugMini),
}

#[enum_dispatch(SmartPlugEnum)]
pub trait SmartPlug: Send {
    fn set_state(
        &mut self,
        state: bool,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send;

    fn get_state(&mut self) -> impl std::future::Future<Output = Result<bool, Error>> + Send;

    fn toggle(&mut self) -> impl std::future::Future<Output = Result<(), Error>> + Send {
        async {
            match self.get_state().await {
                Ok(prev) => self.set_state(!prev).await,
                Err(err) => Err(err),
            }
        }
    }

    fn peripheral(&self) -> &Peripheral;
}

pub trait ConcreteSmartPlug: SmartPlug + Sized {
    fn check_event(event: &CentralEvent) -> bool;

    fn from_peripheral(
        peripheral: Peripheral,
    ) -> impl std::future::Future<Output = Result<Self, Error>> + Send;
}

pub async fn scan_and_connect(
    central: &Adapter,
    condition: impl Fn(&Peripheral) -> bool,
) -> Result<SmartPlugEnum, Error> {
    central.start_scan(ScanFilter::default()).await?;

    let mut events = central.events().await?;
    let mut device: Option<SmartPlugEnum> = None;
    while let Some(event) = events.next().await {
        match event.clone() {
            CentralEvent::DeviceDiscovered(id)
            | CentralEvent::DeviceUpdated(id)
            | CentralEvent::DeviceConnected(id)
            | CentralEvent::ManufacturerDataAdvertisement { id, .. }
            | CentralEvent::ServiceDataAdvertisement { id, .. }
            | CentralEvent::ServicesAdvertisement { id, .. } => {
                let Ok(peripheral) = central.peripheral(&id).await else {
                    continue;
                };

                if !condition(&peripheral) {
                    continue;
                }

                if switchbot::plug_mini::PlugMini::check_event(&event) {
                    peripheral.connect().await?;
                    if let Ok(d) = switchbot::plug_mini::PlugMini::from_peripheral(peripheral).await {
                        device = Some(d.into());
                        break;
                    }
                }
            }
            _ => (),
        }
    }

    central.stop_scan().await?;

    Ok(device.unwrap()) // `device` must be `Some` because `events` is an endless stream
}
