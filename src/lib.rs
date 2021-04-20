use std::{ffi::OsString, fmt, fs, os::unix::ffi::OsStrExt, path::Path};

use anyhow::Context;

#[derive(Debug, Clone)]
pub struct Device {
    pub port: OsString,
    pub name: String,
    pub online: bool,
}

impl Device {
    /// Turns the device on.
    pub fn on(&self) -> anyhow::Result<()> {
        let path = Path::new("/sys/bus/usb/drivers/usb/bind");
        fs::write(path, self.port.as_os_str().as_bytes())
            .with_context(|| format!("Unable to write to {}", path.display()))
    }

    /// Turns the device off.
    pub fn off(&self) -> anyhow::Result<()> {
        let path = Path::new("/sys/bus/usb/drivers/usb/unbind");
        fs::write(path, self.port.as_os_str().as_bytes())
            .with_context(|| format!("Unable to write to {}", path.display()))
    }

    /// Checks if either device name or device port matches the `search` string.
    pub fn matches(&self, search: &str, exact: bool) -> bool {
        let port = self.port.to_string_lossy();
        if exact {
            port == search || self.name == search
        } else {
            port.contains(search) || self.name.contains(search)
        }
    }
}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let active = if self.online { " active " } else { "inactive" };
        let port = self.port.to_string_lossy();
        write!(
            f,
            r#"{port:<5} ({active}): {name}"#,
            name = self.name,
            port = port,
            active = active
        )
    }
}

/// Finds a device which port or name matches the search string.
pub fn find_device(search: &str, exact: bool) -> anyhow::Result<Option<Device>> {
    for device in discover_devices()? {
        let device = device?;
        if device.matches(search, exact) {
            return Ok(Some(device));
        }
    }
    Ok(None)
}

/// Discovers all the available devices.
pub fn discover_devices() -> anyhow::Result<impl Iterator<Item = anyhow::Result<Device>>> {
    let base_path = Path::new("/sys/bus/usb/devices/");
    Ok(fs::read_dir(base_path)
        .with_context(|| format!("Unable to open {}", base_path.display()))?
        .map(move |entry| {
            let entry = entry
                .with_context(|| format!("Unable to get an entry from {}", base_path.display()))?;
            let path = entry.path();
            let metadata = fs::metadata(&path).with_context(|| {
                format!("Unable to get file type of an entry {}", path.display())
            })?;
            if !metadata.is_dir() {
                // Skip non-directories.
                return Ok(None);
            }
            let product_file = path.join("product");
            if !product_file.exists() {
                return Ok(None);
            }
            let contents = std::fs::read_to_string(&product_file).with_context(|| {
                format!("Unable to read product file {}", product_file.display())
            })?;
            let port = path.file_name().expect("There must be a name");
            let driver = Path::new("/sys/bus/usb/drivers/usb/").join(port);
            Ok(Some(Device {
                port: port.into(),
                name: contents.trim().into(),
                online: driver.exists(),
            }))
        })
        .filter_map(Result::transpose))
}
