//! Device actions helper.

use snafu::{ResultExt, Snafu};

use crate::device::{Device, DiscoveryError, Status, StatusError};

mod filter;

pub use filter::{ChainFilter, ClosureFilter, Filter, FilterExt, NoOpFilter};

/// An action applied to a device.
#[derive(Debug, Clone, Copy)]
pub enum Action {
    /// Turns device on.
    On,
    /// Turns device off.
    Off,
    /// When device is off, turns it on; when device is on, turns it off.
    Toggle,
}

/// Applies an action to a list of devices.
pub struct Apply<Devices, Filter>
where
    Devices: IntoIterator<Item = Result<Device, DiscoveryError>>,
{
    devices: Devices,
    filter: Filter,
    dry_run: bool,
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
            dry_run: false,
        }
    }
}

impl<Devices, F> Apply<Devices, F>
where
    Devices: IntoIterator<Item = Result<Device, DiscoveryError>>,
{
    /// Applies a filter to the the device list.
    pub fn filter<NewFilter>(self, filter: NewFilter) -> Apply<Devices, NewFilter>
    where
        NewFilter: Filter,
    {
        Apply {
            devices: self.devices,
            filter,
            dry_run: self.dry_run,
        }
    }

    /// Enables or disabled "dry run".
    ///
    /// When "dry run" is enabled, no real actions are performed.
    pub fn dry_run(self, dry_run: bool) -> Self {
        Apply { dry_run, ..self }
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

impl<Devices, F> Apply<Devices, F>
where
    Devices: IntoIterator<Item = Result<Device, DiscoveryError>>,
    F: Filter,
{
    /// Applies the given action to devices.
    pub fn run(mut self, action: Action) -> Result<(), Error> {
        for device in self.devices.into_iter() {
            let device = device.context(Fetch)?;
            if !self.filter.filter(&device) {
                log::debug!("Skipped {}", device);
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
                (Action::On, Status::Offline) => {
                    log::info!("Turning on {}", device.name);
                    if !self.dry_run {
                        device.on().context(TurnOn { device })?
                    }
                }
                (Action::Off, Status::Online) => {
                    log::info!("Turning off {}", device.name);
                    if !self.dry_run {
                        device.off().context(TurnOff { device })?
                    }
                }
                (Action::Off, Status::Offline) => {
                    log::warn!(
                        r#"Refusing to turn off an inactive device "{}" at {:?}"#,
                        device.name,
                        device.port
                    );
                }
                (Action::Toggle, Status::Online) => {
                    log::info!("Turning off {}", device.name);
                    if !self.dry_run {
                        device.off().context(TurnOff { device })?
                    }
                }
                (Action::Toggle, Status::Offline) => {
                    log::info!("Turning on {}", device.name);
                    if !self.dry_run {
                        device.on().context(TurnOn { device })?
                    }
                }
            }
        }
        Ok(())
    }
}
