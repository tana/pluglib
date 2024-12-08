//! Driver for SwitchBot Plug Mini
//!
//! Reference:
//!     - https://github.com/OpenWonderLabs/SwitchBotAPI-BLE/blob/latest/README.md
//!     - https://github.com/OpenWonderLabs/SwitchBotAPI-BLE/blob/latest/devicetypes/plugmini.md

use std::fmt::Debug;

use btleplug::api::{CentralEvent, Characteristic, Peripheral as _, WriteType};
use btleplug::platform::Peripheral;
use tokio::sync::mpsc::{self, Receiver};
use tokio::task::JoinHandle;
use tokio_stream::StreamExt;
use uuid::{uuid, Uuid};

use crate::{BleSmartPlug, SmartPlug};

const SVC_DATA_UUID: Uuid = uuid!("0000fd3d-0000-1000-8000-00805f9b34fb");
const SVC_UUID: Uuid = uuid!("cba20d00-224d-11e6-9fb8-0002a5d5c51b");
const CHR_UUID_RX: Uuid = uuid!("cba20002-224d-11e6-9fb8-0002a5d5c51b");
const CHR_UUID_TX: Uuid = uuid!("cba20003-224d-11e6-9fb8-0002a5d5c51b");
const CMD_EXPANSION: u8 = 0x0F;

/// Represents a SwitchBot Plug Mini device
pub struct PlugMini {
    peripheral: Peripheral,
    rx_chr: Characteristic,
    #[allow(dead_code)]
    notification_task_handle: JoinHandle<()>,
    chan_rx: Receiver<Vec<u8>>,
}

impl PlugMini {
    async fn send_request(&mut self, cmd: u8, payload: &[u8]) -> Result<Vec<u8>, crate::Error> {
        let mut packet = Vec::with_capacity(2 + payload.len());
        packet.push(0x57); // Magic number
        packet.push((0b00 << 6) | (cmd & 0b1111)); // Header
        packet.extend(payload);

        self.peripheral
            .write(&self.rx_chr, &packet, WriteType::WithResponse)
            .await?;

        Ok(self.chan_rx.recv().await.expect("Channel broken"))
    }
}

impl SmartPlug for PlugMini {
    async fn set_state(&mut self, state: bool) -> Result<(), crate::Error> {
        let payload = if state {
            [0x50, 0x01, 0x01, 0x80]
        } else {
            [0x50, 0x01, 0x01, 0x00]
        };

        let res = self.send_request(CMD_EXPANSION, &payload).await?;

        if res[0] != 0x01 {
            Err(crate::Error::Protocol("Invalid response".to_owned()))
        } else {
            Ok(())
        }
    }

    async fn get_state(&mut self) -> Result<bool, crate::Error> {
        let payload = [0x51, 0x01];

        let res = self.send_request(CMD_EXPANSION, &payload).await?;

        if res[0] != 0x01 {
            Err(crate::Error::Protocol("Invalid response".to_owned()))
        } else {
            match res[1] {
                0x00 => Ok(false),
                0x80 => Ok(true),
                _ => Err(crate::Error::Protocol("Invalid response".to_owned())),
            }
        }
    }
}

impl BleSmartPlug for PlugMini {
    fn check_event(event: &CentralEvent) -> bool {
        match event {
            CentralEvent::ServiceDataAdvertisement {
                id: _,
                service_data,
            } if service_data.contains_key(&SVC_DATA_UUID) => true,
            _ => false,
        }
    }

    async fn connect(peripheral: Peripheral) -> Result<Self, crate::Error> {
        peripheral.connect().await?;

        peripheral.discover_services().await?;
        let services = peripheral.services();
        let Some(service) = services.iter().find(|s| s.uuid == SVC_UUID) else {
            return Err(crate::Error::Protocol(
                "Plug Mini service not found".to_owned(),
            ));
        };

        let Some(tx_chr) = service
            .characteristics
            .iter()
            .find(|c| c.uuid == CHR_UUID_TX)
        else {
            return Err(crate::Error::Protocol(
                "TX characteristic not found".to_owned(),
            ));
        };
        let Some(rx_chr) = service
            .characteristics
            .iter()
            .find(|c| c.uuid == CHR_UUID_RX)
        else {
            return Err(crate::Error::Protocol(
                "RX characteristic not found".to_owned(),
            ));
        };

        // Response is sent through the TX characteristic
        // (i.e. RX and TX is defined as seen from the device)
        peripheral.subscribe(tx_chr).await?;
        let (chan_tx, chan_rx) = mpsc::channel(1);
        let mut notifications = peripheral.notifications().await?;
        let notification_task_handle = tokio::spawn(async move {
            while let Some(notification) = notifications.next().await {
                if notification.uuid == CHR_UUID_TX {
                    chan_tx.send(notification.value).await.unwrap();
                }
            }
        });

        Ok(Self {
            peripheral,
            rx_chr: rx_chr.clone(),
            notification_task_handle,
            chan_rx,
        })
    }

    async fn disconnect(self) -> Result<Peripheral, crate::Error> {
        self.peripheral.disconnect().await?;
        Ok(self.peripheral)
    }
}

impl Debug for PlugMini {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PlugMini({})", self.peripheral.address())?;

        Ok(())
    }
}
