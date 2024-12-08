use btleplug::{
    api::{Central as _, CentralEvent, ScanFilter},
    platform::{Adapter, Peripheral},
};
use enum_dispatch::enum_dispatch;
use tokio_stream::StreamExt as _;

mod switchbot;

#[derive(Debug)]
pub enum Error {
    Protocol(String),
    Ble(btleplug::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            &Error::Protocol(str) => write!(f, "{}", str),
            &Error::Ble(err) => write!(f, "BLE error: {}", err),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self {
            &Error::Ble(err) => Some(err),
            _ => None,
        }
    }
}

impl From<btleplug::Error> for Error {
    fn from(value: btleplug::Error) -> Self {
        Error::Ble(value)
    }
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
}

pub trait BleSmartPlug: SmartPlug + Sized {
    fn check_event(event: &CentralEvent) -> bool;

    fn connect(
        peripheral: Peripheral,
    ) -> impl std::future::Future<Output = Result<Self, Error>> + Send;

    fn disconnect(self) -> impl std::future::Future<Output = Result<Peripheral, Error>> + Send;
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
                    if let Ok(d) = switchbot::plug_mini::PlugMini::connect(peripheral).await {
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
