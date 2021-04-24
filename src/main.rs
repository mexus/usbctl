use anyhow::Context;
use display_error_chain::DisplayErrorChain;
use log::LevelFilter;
use nix::unistd::{getuid, setegid, seteuid, Gid, Uid};
use structopt::StructOpt;
use usbctl::actions::Action;

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

    #[structopt(subcommand)]
    command: Command,
}

/// Command.
#[derive(StructOpt)]
enum Command {
    /// List available devices.
    List,
    /// Turn on a device.
    On {
        /// A search string. It is matches against both port and device name.
        search: Vec<String>,

        /// Matches only when port or name matches the search string exactly.
        #[structopt(short, long)]
        exact: bool,
    },
    /// Turn off a device.
    Off {
        /// A search string. It is matches against both port and device name.
        search: Vec<String>,

        /// Matches only when port or name matches the search string exactly.
        #[structopt(short, long)]
        exact: bool,
    },
    /// Toggles a device.
    Toggle {
        /// A search string. It is matches against both port and device name.
        search: Vec<String>,

        /// Matches only when port or name matches the search string exactly.
        #[structopt(short, long)]
        exact: bool,
    },
}

fn main() {
    setup_logs();
    if let Err(e) = run() {
        log::error!("Terminating with error: {}", DisplayErrorChain::new(&*e));
        std::process::exit(1)
    }
}

fn setup_logs() {
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
    let infos = fern::Dispatch::new()
        .level(LevelFilter::Info)
        .filter(|meta| meta.level() > log::Level::Warn)
        .chain(fern::Output::stdout("\n"));
    fern::Dispatch::new()
        .chain(errors)
        .chain(infos)
        .apply()
        .expect("Should not fail");
}

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

fn drop_privileges(uid: libc::uid_t, gid: libc::gid_t) -> anyhow::Result<()> {
    let uid = Uid::from_raw(uid);
    let gid = Gid::from_raw(gid);

    setegid(gid).with_context(|| format!("Unable to set effective group id to {}", gid))?;
    seteuid(uid).with_context(|| format!("Unable to set effective user id to {}", uid))?;

    Ok(())
}

fn run() -> anyhow::Result<()> {
    let options = Options::from_args();
    if !options.root_is_ok && getuid().is_root() {
        match (options.uid, options.gid) {
            (Some(uid), Some(gid)) => {
                use caps::Capability::*;
                set_capabilities(&(maplit::hashset![CAP_DAC_OVERRIDE, CAP_SETGID, CAP_SETUID]))
                    .context("Unable to set capabilities (phase 1)")?;
                drop_privileges(uid, gid).context("Unable to drop privileges")?;
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
                log::info!("{}", device);
            }
        }
        Command::On { search, exact } => apply(Action::On, search, exact)?,
        Command::Off { search, exact } => apply(Action::Off, search, exact)?,
        Command::Toggle { search, exact } => apply(Action::Toggle, search, exact)?,
    }
    Ok(())
}

fn apply(action: Action, search: Vec<String>, exact: bool) -> anyhow::Result<()> {
    usbctl::actions::Apply::new(usbctl::device::discover().context("Looking for devices")?)
        .filter(|device| search.iter().any(|search| device.matches(search, exact)))
        .run(action)?;
    Ok(())
}
