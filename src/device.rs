//! Device manipulation utilities.

use std::{
    ffi::OsString,
    fmt, fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use snafu::{ResultExt, Snafu};

/// Device status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Device is online.
    Online,
    /// Device is offline.
    Offline,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Status::Online => write!(f, "online"),
            Status::Offline => write!(f, "offline"),
        }
    }
}

impl Status {
    fn from_bool(online: bool) -> Self {
        if online {
            Self::Online
        } else {
            Self::Offline
        }
    }
}

/// A USB device.
#[derive(Debug, Clone)]
pub struct Device {
    /// USB port.
    pub port: OsString,
    /// Device name.
    pub name: String,
    /// Whether device is online.
    pub online: Status,
}

/// Device status change error.
#[derive(Debug, Snafu)]
pub enum StatusError {
    /// Status save error.
    #[snafu(display("Unable to write status ON to {}", path.display()))]
    On {
        /// Directory of interest.
        path: PathBuf,

        /// Source error.
        source: std::io::Error,
    },
    /// Status save error.
    #[snafu(display("Unable to write status OFF to {}", path.display()))]
    Off {
        /// Directory of interest.
        path: PathBuf,

        /// Source error.
        source: std::io::Error,
    },
}

impl Device {
    /// Turns the device on.
    pub fn on(&self) -> Result<(), StatusError> {
        let path = Path::new("/sys/bus/usb/drivers/usb/bind");
        fs::write(path, self.port.as_os_str().as_bytes()).context(On { path })
    }

    /// Turns the device off.
    pub fn off(&self) -> Result<(), StatusError> {
        let path = Path::new("/sys/bus/usb/drivers/usb/unbind");
        fs::write(path, self.port.as_os_str().as_bytes()).context(Off { path })
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
        let port = self.port.to_string_lossy();
        write!(
            f,
            r#"{port:<5} ({active}): {name}"#,
            name = self.name,
            port = port,
            active = self.online
        )
    }
}

/// Directory open error.
#[derive(Debug, Snafu)]
pub enum DirectoryOpenError {
    /// Unable to open a directory.
    #[snafu(display("Unable to open {}", path.display()))]
    OpenDir {
        /// Directory of interest.
        path: PathBuf,

        /// Source error.
        source: std::io::Error,
    },
}

/// Device discovery error.
#[derive(Debug, Snafu)]
pub enum DiscoveryError {
    /// Error while fetching a directory entry.
    #[snafu(display("Unable to fetch an entry from {}", path.display()))]
    FetchEntry {
        /// Directory of interest.
        path: PathBuf,

        /// Source error.
        source: std::io::Error,
    },
    /// Unable to get metadata of a file.
    #[snafu(display("Unable to get metadata of {}", path.display()))]
    Metadata {
        /// File path.
        path: PathBuf,

        /// Source error.
        source: std::io::Error,
    },

    /// Unable to read a product file.
    #[snafu(display("Unable to read product file: {}", path.display()))]
    ReadFile {
        /// File path.
        path: PathBuf,

        /// Source error.
        source: std::io::Error,
    },
}

/// Discovers all the available devices.
pub fn discover() -> Result<impl Iterator<Item = Result<Device, DiscoveryError>>, DirectoryOpenError>
{
    let base_path = Path::new("/sys/bus/usb/devices/");
    Ok(fs::read_dir(base_path)
        .context(OpenDir { path: base_path })?
        .map(move |entry| {
            let entry = entry.context(FetchEntry { path: base_path })?;
            let path = entry.path();
            let path = &path;
            let metadata = match fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // Skip disappeared device
                    return Ok(None);
                }
                Err(source) => {
                    return Err(DiscoveryError::Metadata {
                        source,
                        path: path.into(),
                    })
                }
            };
            if !metadata.is_dir() {
                // Skip non-directories.
                return Ok(None);
            }
            let product_file = path.join("product");
            if !product_file.exists() {
                return Ok(None);
            }
            let contents = std::fs::read_to_string(&product_file).context(ReadFile { path })?;
            let port = path.file_name().expect("There must be a name");
            let driver = Path::new("/sys/bus/usb/drivers/usb/").join(port);
            Ok(Some(Device {
                port: port.into(),
                name: contents.trim().into(),
                online: Status::from_bool(driver.exists()),
            }))
        })
        .filter_map(Result::transpose))
}
