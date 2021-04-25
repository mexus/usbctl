use anyhow::Context;
use display_error_chain::DisplayErrorChain;
use log::LevelFilter;
use nix::unistd::{getuid, setegid, seteuid, Gid, Uid};
use structopt::StructOpt;
use usbctl::actions::{Action, Filter};

/// USB devices management.
#[derive(StructOpt)]
struct Options {
    /// Suppress a warning when running as root.
    #[structopt(long = "i-know-what-i-am-doing")]
    root_is_ok: bool,

    /// When run as root, drop privileges to the given user id.
    #[structopt(short = "u", env = "SUDO_UID", conflicts_with("root-is-ok"))]
    uid: Option<libc::uid_t>,

    /// When run as root, drop privileges to the given group id.
    #[structopt(short = "g", env = "SUDO_GID", requires("uid"))]
    gid: Option<libc::gid_t>,

    /// Debug output.
    #[structopt(short, long)]
    debug: bool,

    /// Enables toggling "host" and "hub" devices.
    #[structopt(long)]
    allow_host: bool,

    #[structopt(subcommand)]
    command: Command,
}

/// Command.
#[derive(StructOpt)]
enum Command {
    /// List available devices.
    List,

    /// Turn on devices.
    On {
        #[structopt(flatten)]
        value: SearchOptions,
    },

    /// Turn off devices.
    Off {
        #[structopt(flatten)]
        value: SearchOptions,
    },

    /// Toggles devices.
    Toggle {
        #[structopt(flatten)]
        value: SearchOptions,
    },
}

#[derive(StructOpt)]
struct SearchOptions {
    /// A search string. It is matches against both port and device name.
    search: Vec<String>,

    /// Matches only when port or name matches the search string exactly.
    #[structopt(short, long)]
    exact: bool,

    /// Enables "dry run" mode, when no real actions are performed.
    #[structopt(long)]
    dry_run: bool,
}

fn main() {
    let options = Options::from_args();
    setup_logs(options.debug);
    if let Err(e) = run(options) {
        log::error!("Terminating with error: {}", DisplayErrorChain::new(&*e));
        std::process::exit(1)
    }
}

fn setup_logs(debug: bool) {
    // Warnings and errors go to stderr.
    let errors = fern::Dispatch::new()
        .level(LevelFilter::Warn)
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}] [{}] {}",
                record.target(),
                record.level(),
                message
            ))
        })
        .chain(fern::Output::stderr("\n"));
    // Informational messages go to stdout.
    let infos = fern::Dispatch::new()
        .level(LevelFilter::Info)
        .filter(|meta| meta.level() > log::Level::Warn)
        .chain(fern::Output::stdout("\n"));
    let mut cumulative_dispatcher = fern::Dispatch::new().chain(errors).chain(infos);
    if debug {
        // Debug messages go to stderr.
        let debug = fern::Dispatch::new()
            .level(LevelFilter::Debug)
            .format(|out, message, record| {
                out.finish(format_args!(
                    "[{}] [{}] {}",
                    record.target(),
                    record.level(),
                    message
                ))
            })
            .filter(|meta| meta.level() >= log::Level::Debug)
            .chain(fern::Output::stdout("\n"));
        cumulative_dispatcher = cumulative_dispatcher.chain(debug);
    }
    cumulative_dispatcher.apply().expect("Should not fail");
}

/// Sets linux capabilities to the current process.
///
/// See `man 7 capabilities` for details.
fn set_capabilities(capabilities: &caps::CapsHashSet) -> anyhow::Result<()> {
    use caps::CapSet::*;
    for &cap_set in &[Effective, Permitted] {
        caps::set(None, cap_set, capabilities).with_context(|| {
            format!(
                "Unable to set {:?} capabilities {:?}",
                cap_set, capabilities
            )
        })?;
    }
    log::debug!("Capabilities updated to {:?}", capabilities);
    Ok(())
}

/// Drops privileges by setting effective user id and group id of the current
/// process to the provided ones.
fn drop_privileges(uid: libc::uid_t, gid: libc::gid_t) -> anyhow::Result<()> {
    debug_assert_ne!(uid, 0);
    debug_assert_ne!(gid, 0);
    let uid = Uid::from_raw(uid);
    let gid = Gid::from_raw(gid);

    setegid(gid).with_context(|| format!("Unable to set effective group id to {}", gid))?;
    seteuid(uid).with_context(|| format!("Unable to set effective user id to {}", uid))?;

    Ok(())
}

fn run(options: Options) -> anyhow::Result<()> {
    if !options.root_is_ok && getuid().is_root() {
        // When running as root, try to drop privileges while leaving the
        // required capabilities.
        match (options.uid, options.gid) {
            (Some(uid), Some(gid)) => {
                use caps::Capability::*;
                // We need to keep CAP_SETGID and CAP_SETUID so far in order to
                // change effective UID and GID.
                set_capabilities(&(maplit::hashset![CAP_DAC_OVERRIDE, CAP_SETGID, CAP_SETUID]))
                    .context("Unable to set capabilities (phase 1)")?;
                drop_privileges(uid, gid).context("Unable to drop privileges")?;
                // Don't need CAP_SETGID and CAP_SETUID anymore.
                set_capabilities(&(maplit::hashset![CAP_DAC_OVERRIDE]))
                    .context("Unable to set capabilities (phase 2)")?;
            }
            (Some(_), None) | (None, Some(_)) => {
                anyhow::bail!("Both uid and gid must be set")
            }
            (None, None) => anyhow::bail!("Running as root, but user id is not set"),
        }
    }
    match options.command {
        Command::List => {
            let devices = usbctl::device::discover()
                .context("Looking for devices")?
                .collect::<Result<Vec<_>, _>>()
                .context("Collecting devices")?;
            log::info!("Found {} device(s):", devices.len());
            for device in devices {
                if !options.allow_host
                    && (device.name.contains("Host") || device.name.contains("host"))
                {
                    continue;
                }
                log::info!("{}", device);
            }
        }
        Command::On { value } => apply(Action::On, value, options.allow_host)?,
        Command::Off { value } => apply(Action::Off, value, options.allow_host)?,
        Command::Toggle { value } => apply(Action::Toggle, value, options.allow_host)?,
    }
    Ok(())
}

/// Applies an action to filtered devices.
fn apply(
    action: Action,
    SearchOptions {
        search,
        exact,
        dry_run,
    }: SearchOptions,
    allow_host: bool,
) -> anyhow::Result<()> {
    usbctl::actions::Apply::new(usbctl::device::discover().context("Looking for devices")?)
        .filter(DeviceMatch::new(search, exact, allow_host))
        .dry_run(dry_run)
        .run(action)?;
    Ok(())
}

/// A simple filter that checks if a device matches any of the search strings.
struct DeviceMatch {
    search: Vec<String>,
    exact: bool,
    allow_host: bool,
}

impl DeviceMatch {
    /// Initializes a [DeviceMatch].
    fn new(search: Vec<String>, exact: bool, allow_host: bool) -> Self {
        Self {
            search,
            exact,
            allow_host,
        }
    }
}

impl Filter for DeviceMatch {
    fn filter(&mut self, device: &usbctl::device::Device) -> bool {
        if self.search.is_empty()
            || !self.allow_host && (device.name.contains("Host") || device.name.contains("host"))
        {
            false
        } else {
            self.search
                .iter()
                .any(|search| device.matches(search, self.exact))
        }
    }
}
