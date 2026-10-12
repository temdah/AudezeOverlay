//! Linux HID access through the kernel's hidraw interface.
//!
//! One hidraw node covers every collection of a USB HID interface, so the vendor
//! control collection is found by reading the report descriptor the kernel
//! publishes in sysfs. Input is fetched with the HID GET_REPORT request, the same
//! mechanism as `HidD_GetInputReport` on Windows; unsolicited reports are only
//! read by diagnostics. No driver, daemon or libudev is involved. Access to
//! `/dev/hidraw*` is granted by a udev rule (see docs/LINUX.md).
use super::{
    control_report_sizes_plausible, Enumeration, Hid, InputStream, Interface, CONTROL_USAGE,
};
use crate::device::{descriptor, profiles::Profile, selection::Candidate};
use rustix::{
    event::{poll, PollFd, PollFlags, Timespec},
    io::Errno,
    ioctl::{self, opcode, Updater},
    net::{
        self,
        netlink::{self, SocketAddrNetlink},
        AddressFamily, RecvFlags, SocketFlags, SocketType,
    },
};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, String>;

const SYS_CLASS_HIDRAW: &str = "/sys/class/hidraw";
const DEV: &str = "/dev";
const VENDOR_ID: u32 = 0x3329;
const USB_BUS: u32 = 0x0003;
/// The kernel reports a device before udev has finished applying its permissions;
/// waiting for a quiet moment keeps the worker from opening a node too early.
const HOTPLUG_SETTLE_MS: u32 = 300;

/// Shown when a recognized headset exists but its hidraw node cannot be opened.
const PERMISSION_HELP: &str = "Permission denied opening the headset's /dev/hidraw device. \
Install the udev rule described in docs/LINUX.md, then reconnect the dongle.";
/// `HIDIOCGINPUT`, which fetches an input report on demand, arrived in Linux 5.11.
const KERNEL_TOO_OLD: &str =
    "This kernel cannot read HID input reports on request. Linux 5.11 or newer is required.";

struct Node {
    file: File,
    /// Largest input report of the control collection, report ID included.
    input_len: usize,
}
impl Hid for Node {
    fn write_report(&self, report: &[u8]) -> Result<()> {
        let written = (&self.file).write(report).map_err(|e| e.to_string())?;
        if written != report.len() {
            return Err("Incomplete USB sound command".into());
        }
        Ok(())
    }
    fn get_input_report(&self, report: &mut [u8; 64]) -> Result<()> {
        const GET_INPUT_REPORT: ioctl::Opcode = opcode::read_write::<[u8; 64]>(b'H', 0x0A);
        // SAFETY: HIDIOCGINPUT reads the report ID from the first byte and fills
        // the same buffer with that report. The opcode encodes exactly the
        // 64-byte array type passed here, so the kernel cannot write past it.
        unsafe {
            ioctl::ioctl(
                &self.file,
                Updater::<{ GET_INPUT_REPORT }, [u8; 64]>::new(report),
            )
        }
        .map_err(|errno| match errno {
            // A kernel without the request answers EINVAL (older hidraw) or ENOTTY.
            Errno::INVAL | Errno::NOTTY => KERNEL_TOO_OLD.to_string(),
            _ => "USB connection lost. Reconnecting…".to_string(),
        })
    }
    fn listen(&self) -> Result<Box<dyn InputStream>> {
        let file = self.file.try_clone().map_err(|e| e.to_string())?;
        Ok(Box::new(Listener {
            file,
            input_len: self.input_len,
        }))
    }
}

struct Listener {
    file: File,
    input_len: usize,
}
impl InputStream for Listener {
    fn read(&mut self, timeout_ms: u32) -> Result<Option<Vec<u8>>> {
        let timeout = Timespec {
            tv_sec: i64::from(timeout_ms / 1000),
            tv_nsec: i64::from(timeout_ms % 1000) * 1_000_000,
        };
        let mut fds = [PollFd::new(&self.file, PollFlags::IN)];
        match poll(&mut fds, Some(&timeout)) {
            Ok(0) | Err(Errno::INTR) => return Ok(None),
            Ok(_) => {}
            Err(e) => return Err(e.to_string()),
        }
        // hidraw drops whatever does not fit, so size the buffer to the report.
        let mut buffer = vec![0u8; self.input_len];
        let count = loop {
            match self.file.read(&mut buffer) {
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                other => break other.map_err(|e| e.to_string())?,
            }
        };
        buffer.truncate(count);
        Ok(Some(buffer))
    }
}

/// Lists the supported Audeze control interfaces the current user can open.
pub(in crate::device) fn enumerate() -> Result<Enumeration> {
    enumerate_in(Path::new(SYS_CLASS_HIDRAW), Path::new(DEV), |path| {
        File::options().read(true).write(true).open(path)
    })
}

/// `enumerate` over explicit directories and an explicit opener, so the
/// discovery rules can be tested against a fake sysfs tree.
fn enumerate_in(
    sys_class: &Path,
    dev: &Path,
    open: impl Fn(&Path) -> std::io::Result<File>,
) -> Result<Enumeration> {
    let mut candidates = Vec::new();
    let mut wired_seen = Vec::new();
    let mut permission_denied = false;
    let Ok(entries) = std::fs::read_dir(sys_class) else {
        // No hidraw class: no HID devices at all, which is not an error.
        return Ok(Enumeration {
            candidates,
            wired_seen,
        });
    };
    let mut names: Vec<_> = entries.flatten().map(|e| e.file_name()).collect();
    names.sort();
    for name in names {
        let node = sys_class.join(&name).join("device");
        let Some(profile) = std::fs::read_to_string(node.join("uevent"))
            .ok()
            .and_then(|text| profile_from_uevent(&text))
        else {
            continue;
        };
        // A wired identity counts as present even if its interface is unusable,
        // exactly as on Windows.
        if profile.connection == crate::model::Connection::WiredUsb
            && !wired_seen.contains(&profile)
        {
            wired_seen.push(profile);
        }
        let Some((input_len, output_len)) = control_lengths(&node.join("report_descriptor")) else {
            continue;
        };
        // HIDIOCGINPUT is issued with a fixed 64-byte buffer (see `get_input_report`),
        // so a longer control report could never be read; skip it instead of
        // letting every read fail later.
        if input_len > 64 {
            continue;
        }
        let path: PathBuf = dev.join(&name);
        match open(&path) {
            Ok(file) => candidates.push(Candidate {
                profile,
                endpoint: Interface {
                    id: path.to_string_lossy().into_owned(),
                    output_len,
                    hid: Box::new(Node { file, input_len }),
                },
            }),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                permission_denied = true;
            }
            // Gone between listing and opening, or not ours to open: skip it.
            Err(_) => {}
        }
    }
    if candidates.is_empty() && permission_denied {
        return Err(PERMISSION_HELP.into());
    }
    Ok(Enumeration {
        candidates,
        wired_seen,
    })
}

/// `HID_ID=0003:00003329:00004B18` -> the supported profile, if this is a USB
/// Audeze product on the allowlist. Unknown products are never guessed at.
fn profile_from_uevent(uevent: &str) -> Option<Profile> {
    let id = uevent.lines().find_map(|l| l.strip_prefix("HID_ID="))?;
    let mut parts = id.split(':').map(|p| u32::from_str_radix(p, 16));
    let (bus, vendor, product) = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    if bus != USB_BUS || vendor != VENDOR_ID {
        return None;
    }
    Profile::from_usb(VENDOR_ID as u16, u16::try_from(product).ok()?)
}

/// Largest input and output report (bytes, report ID included) of the vendor
/// control collection, if the interface has one with plausible report sizes.
fn control_lengths(descriptor_path: &Path) -> Option<(usize, usize)> {
    let mut bytes = Vec::new();
    File::open(descriptor_path)
        .ok()?
        .take(descriptor::MAX_DESCRIPTOR_BYTES as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    descriptor::collections(&bytes)
        .into_iter()
        .find(|c| (c.usage_page, c.usage) == CONTROL_USAGE)
        .filter(|c| control_report_sizes_plausible(c.max_input_bytes, c.max_output_bytes))
        .map(|c| (c.max_input_bytes, c.max_output_bytes))
}

/// Calls `on_change` whenever the kernel reports a hidraw device appearing or
/// disappearing. The thread sleeps in `recv` until the kernel wakes it, so it
/// costs nothing while idle. If the socket cannot be opened (for example in a
/// restrictive sandbox) hot-plug is simply not accelerated: the worker still
/// retries opening the device on its normal reconnect schedule.
pub(in crate::device) fn watch_hotplug(on_change: impl Fn() + Send + 'static) {
    let _ = std::thread::Builder::new()
        .name("hid-hotplug".into())
        .spawn(move || {
            let _ = watch(on_change);
        });
}

fn watch(on_change: impl Fn()) -> rustix::io::Result<()> {
    let socket = net::socket_with(
        AddressFamily::NETLINK,
        SocketType::RAW,
        SocketFlags::CLOEXEC,
        Some(netlink::KOBJECT_UEVENT),
    )?;
    // Group 1 carries the kernel's own events; reading it needs no privilege.
    net::bind(&socket, &SocketAddrNetlink::new(0, 1))?;
    let mut buffer = [0u8; 4096];
    loop {
        match net::recv(&socket, &mut buffer, RecvFlags::empty()) {
            Ok((received, _)) if is_hidraw_change(&buffer[..received.min(buffer.len())]) => {}
            Ok(_) | Err(Errno::INTR) => continue,
            // A burst overran the socket buffer, so events were lost: assume a change.
            Err(Errno::NOBUFS) => {}
            Err(e) => return Err(e),
        }
        settle(&socket, &mut buffer)?;
        on_change();
    }
}

/// Waits until no further event arrives for `HOTPLUG_SETTLE_MS`, so one replug
/// (several kernel events) wakes the worker once, after udev has had time to
/// grant access to the new node.
fn settle(socket: &impl rustix::fd::AsFd, buffer: &mut [u8]) -> rustix::io::Result<()> {
    let quiet = Timespec {
        tv_sec: 0,
        tv_nsec: i64::from(HOTPLUG_SETTLE_MS) * 1_000_000,
    };
    loop {
        let mut fds = [PollFd::new(socket, PollFlags::IN)];
        match poll(&mut fds, Some(&quiet)) {
            Ok(0) => return Ok(()),
            Ok(_) => match net::recv(socket, &mut *buffer, RecvFlags::DONTWAIT) {
                Ok(_) | Err(Errno::INTR | Errno::NOBUFS | Errno::AGAIN) => {}
                Err(e) => return Err(e),
            },
            Err(Errno::INTR) => {}
            Err(e) => return Err(e),
        }
    }
}

/// A kernel uevent is `action@devpath\0KEY=value\0...`; hot-plug cares about
/// hidraw nodes being added or removed.
fn is_hidraw_change(message: &[u8]) -> bool {
    let mut fields = message.split(|b| *b == 0);
    let header = fields.next().unwrap_or_default();
    let added_or_removed = header.starts_with(b"add@") || header.starts_with(b"remove@");
    added_or_removed && fields.any(|f| f == b"SUBSYSTEM=hidraw")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Connection, HeadsetFamily};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A reduced, hand-written descriptor with the same vendor collection (usage
    /// page ff13, report 6 out and 7 in, 61 bytes each) as the captured one tested
    /// in `descriptor.rs`. It is a synthetic fixture, not hardware evidence.
    const VENDOR_DESCRIPTOR: &[u8] = &[
        0x05, 0x0c, 0x09, 0x01, 0xa1, 0x01, 0x85, 0x0c, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02, 0xc0,
        0x06, 0x13, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x85, 0x06, 0x75, 0x08, 0x95, 0x3d, 0x91, 0x02,
        0x85, 0x07, 0x95, 0x3d, 0x81, 0x02, 0xc0,
    ];

    /// A fake `/sys/class/hidraw` and `/dev` that deletes itself.
    struct Tree(PathBuf);
    impl Tree {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "audeze-hidraw-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(root.join("sys")).unwrap();
            std::fs::create_dir_all(root.join("dev")).unwrap();
            Self(root)
        }
        fn node(&self, name: &str, hid_id: &str, descriptor: &[u8]) {
            let device = self.0.join("sys").join(name).join("device");
            std::fs::create_dir_all(&device).unwrap();
            std::fs::write(
                device.join("uevent"),
                format!("DRIVER=hid-generic\nHID_ID={hid_id}\n"),
            )
            .unwrap();
            std::fs::write(device.join("report_descriptor"), descriptor).unwrap();
            std::fs::write(self.0.join("dev").join(name), b"").unwrap();
        }
        fn enumerate(&self, open: impl Fn(&Path) -> std::io::Result<File>) -> Result<Enumeration> {
            enumerate_in(&self.0.join("sys"), &self.0.join("dev"), open)
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn open_plain(path: &Path) -> std::io::Result<File> {
        File::open(path)
    }

    #[test]
    fn the_control_interface_of_a_known_dongle_is_found_with_its_report_length() {
        let tree = Tree::new();
        tree.node("hidraw0", "0003:00003329:00004B18", VENDOR_DESCRIPTOR);
        let found = tree.enumerate(open_plain).unwrap();
        assert_eq!(found.candidates.len(), 1);
        let candidate = &found.candidates[0];
        assert_eq!(candidate.profile.family, HeadsetFamily::Maxwell);
        assert_eq!(candidate.profile.connection, Connection::WirelessDongle);
        assert_eq!(candidate.endpoint.output_len, 62);
        assert!(candidate.endpoint.id.ends_with("/dev/hidraw0"));
    }

    #[test]
    fn other_vendors_unknown_products_and_non_usb_buses_are_ignored() {
        let tree = Tree::new();
        tree.node("hidraw0", "0003:0000046D:0000C52B", VENDOR_DESCRIPTOR);
        tree.node("hidraw1", "0003:00003329:00004BA7", VENDOR_DESCRIPTOR);
        tree.node("hidraw2", "0005:00003329:00004BA9", VENDOR_DESCRIPTOR);
        tree.node("hidraw3", "garbage", VENDOR_DESCRIPTOR);
        let found = tree.enumerate(open_plain).unwrap();
        assert!(found.candidates.is_empty() && found.wired_seen.is_empty());
    }

    #[test]
    fn an_interface_without_the_control_collection_is_not_a_candidate() {
        let tree = Tree::new();
        // Consumer controls only: same product, wrong collection.
        tree.node(
            "hidraw0",
            "0003:00003329:00004BAA",
            &VENDOR_DESCRIPTOR[..15],
        );
        let found = tree.enumerate(open_plain).unwrap();
        assert!(found.candidates.is_empty());
        // The wired identity is still noticed, so the cable is known to be present.
        assert_eq!(found.wired_seen.len(), 1);
    }

    #[test]
    fn a_denied_node_is_a_permission_problem_not_an_absent_device() {
        let tree = Tree::new();
        tree.node("hidraw0", "0003:00003329:00004BA9", VENDOR_DESCRIPTOR);
        let denied = |_: &Path| Err(std::io::ErrorKind::PermissionDenied.into());
        let error = tree.enumerate(denied).err().expect("must fail");
        assert!(error.contains("udev rule"), "{error}");
        // A nonexistent class directory (no HID devices) is simply empty.
        let none = enumerate_in(
            Path::new("/nonexistent/hidraw"),
            Path::new("/dev"),
            open_plain,
        );
        assert!(none.unwrap().candidates.is_empty());
    }

    #[test]
    fn one_denied_node_does_not_hide_a_usable_one() {
        let tree = Tree::new();
        tree.node("hidraw0", "0003:00003329:00004BAA", VENDOR_DESCRIPTOR);
        tree.node("hidraw1", "0003:00003329:00004BA9", VENDOR_DESCRIPTOR);
        let open = |path: &Path| {
            if path.ends_with("hidraw0") {
                Err(std::io::ErrorKind::PermissionDenied.into())
            } else {
                File::open(path)
            }
        };
        let found = tree.enumerate(open).unwrap();
        assert_eq!(found.candidates.len(), 1);
        assert_eq!(found.candidates[0].profile.product_id, 0x4ba9);
        assert_eq!(found.wired_seen.len(), 1);
    }

    #[test]
    fn only_added_or_removed_hidraw_devices_wake_the_worker() {
        let event = |header: &str, subsystem: &str| {
            format!("{header}\0ACTION=x\0SUBSYSTEM={subsystem}\0MAJOR=241\0").into_bytes()
        };
        assert!(is_hidraw_change(&event(
            "add@/devices/usb1/hidraw/hidraw0",
            "hidraw"
        )));
        assert!(is_hidraw_change(&event(
            "remove@/devices/usb1/hidraw/hidraw0",
            "hidraw"
        )));
        assert!(!is_hidraw_change(&event(
            "change@/devices/usb1/hidraw/hidraw0",
            "hidraw"
        )));
        assert!(!is_hidraw_change(&event("add@/devices/usb1", "usb")));
        assert!(!is_hidraw_change(b"libudev\0\xfe\xed\xca\xfe"));
        assert!(!is_hidraw_change(b""));
    }

    /// Manual check with real hardware: run it with
    /// `cargo test -- --ignored replugged_dongle --nocapture`, then unplug and
    /// replug the dongle within 90 seconds. The watcher must wake, and the dongle
    /// must be enumerable again once udev has granted access to the new node.
    #[test]
    #[ignore = "needs a person to replug the dongle"]
    fn a_replugged_dongle_wakes_the_watcher_and_is_found_again() {
        use std::time::{Duration, Instant};
        let (sender, receiver) = std::sync::mpsc::channel();
        watch_hotplug(move || {
            let _ = sender.send(());
        });
        eprintln!("Unplug and replug the dongle now (90 s).");
        let started = Instant::now();
        receiver
            .recv_timeout(Duration::from_secs(90))
            .expect("no hidraw event arrived within 90 s");
        eprintln!("watcher woke after {:?}", started.elapsed());
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match enumerate() {
                Ok(found) if !found.candidates.is_empty() => break,
                other if Instant::now() > deadline => {
                    panic!("dongle not found again: {:?}", other.err())
                }
                _ => std::thread::sleep(Duration::from_millis(200)),
            }
        }
        eprintln!("found again after {:?}", started.elapsed());
    }
}
