//! Device actions helper.

use snafu::{ResultExt, Snafu};

use crate::device::{Device, DiscoveryError, Status, StatusError};

/// An action applied to a device.
#[derive(Debug, Clone, Copy)]
pub enum Action {
    /// Turns device on.
    On,
    /// Turns device off.
    Off,
    /// When device is off, turns it off; when device is on, turns it on.
    Toggle,
}

/// Applies an action to a list of devices.
pub struct Apply<Devices, Filter>
where
    Devices: IntoIterator<Item = Result<Device, DiscoveryError>>,
{
    devices: Devices,
    filter: Filter,
}

/// Device filter trait.
pub trait Filter {
    /// Checks if the given device should be yielded.
    fn filter(&mut self, device: &Device) -> bool;
}

/// A no-op filter which yields all the supplied devices.
#[derive(Debug, Default)]
pub struct NoOpFilter(());

impl Filter for NoOpFilter {
    #[inline]
    fn filter(&mut self, _device: &Device) -> bool {
        true
    }
}

impl<F: FnMut(&Device) -> bool> Filter for F {
    #[inline]
    fn filter(&mut self, device: &Device) -> bool {
        (self)(device)
    }
}

impl<Devices> Apply<Devices, NoOpFilter>
where
    Devices: IntoIterator<Item = Result<Device, DiscoveryError>>,
{
    /// Creates an [Action] instance with the given devices.
    pub fn new(devices: Devices) -> Self {
        Apply {
            devices,
            filter: NoOpFilter::default(),
        }
    }

    /// Applies a filter to the the device list.
    pub fn filter<Filter>(self, filter: Filter) -> Apply<Devices, Filter>
    where
        Filter: FnMut(&Device) -> bool,
    {
        Apply {
            devices: self.devices,
            filter,
        }
    }
}

/// Operation error.
#[derive(Debug, Snafu)]
pub enum Error {
    /// Fetching a device.
    #[snafu(display("Fetching a device"))]
    Fetch {
        /// Source error
        source: DiscoveryError,
    },

    /// Unable to turn a device on.
    #[snafu(display("Unable to turn on {}", device))]
    TurnOn {
        /// Device in trouble.
        device: Device,
        /// Source error.
        source: StatusError,
    },

    /// Unable to turn a device off.
    #[snafu(display("Unable to turn off {}", device))]
    TurnOff {
        /// Device in trouble.
        device: Device,
        /// Source error.
        source: StatusError,
    },
}

impl<Devices, Filter> Apply<Devices, Filter>
where
    Devices: IntoIterator<Item = Result<Device, DiscoveryError>>,
    Filter: FnMut(&Device) -> bool,
{
    /// Applies the given action to devices.
    pub fn run(mut self, action: Action) -> Result<(), Error> {
        for device in self.devices.into_iter() {
            let device = device.context(Fetch)?;
            if (self.filter)(&device) {
                log::debug!("Skipping {}", device);
                continue;
            }
            match (action, device.online) {
                (Action::On, Status::Online) => {
                    log::warn!(
                        r#"Refusing to turn on an active device "{}" at {:?}"#,
                        device.name,
                        device.port
                    );
                }
                (Action::On, Status::Offline) => device.on().context(TurnOn { device })?,
                (Action::Off, Status::Online) => device.off().context(TurnOff { device })?,
                (Action::Off, Status::Offline) => {
                    log::warn!(
                        r#"Refusing to turn off an inactive device "{}" at {:?}"#,
                        device.name,
                        device.port
                    );
                }
                (Action::Toggle, Status::Online) => device.off().context(TurnOff { device })?,
                (Action::Toggle, Status::Offline) => device.on().context(TurnOn { device })?,
            }
        }
        Ok(())
    }
}
