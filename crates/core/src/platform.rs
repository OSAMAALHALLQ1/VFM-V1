//! Windows shell integration helpers.
//!
//! Pure descriptors and a loopback-based single-instance hand-off live here so
//! they can be tested without a desktop session. The actual WinRT media-session
//! bridge and global media-key hook are intentionally not implemented yet (see
//! `docs/testing.md`); they need a separate, approved platform crate because
//! this workspace confines handwritten `unsafe` to `vlc-wrapper`.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;

use crate::brand::{APP_ID, APP_NAME, EXE_NAME, PROG_ID, SUPPORTED_EXTENSIONS};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryEntry {
    /// HKCU-relative key path.
    pub key: String,
    /// Empty string means the key's default value.
    pub name: String,
    pub value: String,
}

/// Registry descriptor for "open with" and file associations under HKCU.
/// Kept as data so it is reviewable and testable without touching the registry.
pub fn association_entries(exe: &Path) -> Vec<RegistryEntry> {
    let exe = exe.to_string_lossy();
    let command = format!("\"{exe}\" \"%1\"");
    let class = format!("Software\\Classes\\{PROG_ID}");
    let mut entries = vec![
        RegistryEntry {
            key: class.clone(),
            name: String::new(),
            value: format!("{APP_NAME} media file"),
        },
        RegistryEntry {
            key: format!("{class}\\shell\\open\\command"),
            name: String::new(),
            value: command.clone(),
        },
        RegistryEntry {
            key: format!("Software\\Classes\\Applications\\{EXE_NAME}.exe\\shell\\open\\command"),
            name: String::new(),
            value: command,
        },
    ];
    for ext in SUPPORTED_EXTENSIONS {
        entries.push(RegistryEntry {
            key: format!("Software\\Classes\\.{ext}"),
            name: String::new(),
            value: PROG_ID.to_owned(),
        });
    }
    entries
}

/// Writes the descriptor under `HKEY_CURRENT_USER` (no elevation required).
#[cfg(windows)]
pub fn apply_file_associations(exe: &Path) -> Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for entry in association_entries(exe) {
        let (key, _) = hkcu.create_subkey(&entry.key)?;
        key.set_value(&entry.name, &entry.value)?;
    }
    Ok(())
}

/// Loopback port derived from the app id; stable across installs, unlikely to
/// collide, and needs no Win32 named-pipe FFI.
pub fn instance_port() -> u16 {
    let hash = APP_ID.bytes().fold(2166136261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16777619)
    });
    40_000 + (hash % 20_000) as u16
}

/// The first instance holds the listener; later instances report themselves via
/// [`Self::is_second_instance`] and forward their media arguments with
/// [`Self::notify`].
pub struct SingleInstance {
    listener: TcpListener,
}

impl SingleInstance {
    pub fn try_acquire() -> io::Result<Option<Self>> {
        Self::bind(instance_port())
    }

    fn bind(port: u16) -> io::Result<Option<Self>> {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => {
                listener.set_nonblocking(true)?;
                Ok(Some(Self { listener }))
            }
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// True when another instance already owns the application port.
    pub fn is_second_instance() -> bool {
        Self::is_second_on(instance_port())
    }

    fn is_second_on(port: u16) -> bool {
        TcpStream::connect(("127.0.0.1", port)).is_ok()
    }

    /// Sends one media path per line to the running instance.
    pub fn notify(media: &[String]) -> io::Result<bool> {
        Self::notify_on(instance_port(), media)
    }

    fn notify_on(port: u16, media: &[String]) -> io::Result<bool> {
        let mut stream = match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => stream,
            Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => return Ok(false),
            Err(e) => return Err(e),
        };
        for item in media {
            stream.write_all(item.as_bytes())?;
            stream.write_all(b"\n")?;
        }
        stream.shutdown(std::net::Shutdown::Write)?;
        Ok(true)
    }

    /// Drains pending hand-offs. Returns one argument list per connecting
    /// instance. Non-blocking: safe to call from the UI event loop.
    pub fn poll(&self) -> Vec<Vec<String>> {
        let mut batches = Vec::new();
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    let mut lines = Vec::new();
                    for line in BufReader::new(stream).lines().map_while(io::Result::ok) {
                        if !line.is_empty() {
                            lines.push(line);
                        }
                    }
                    if !lines.is_empty() {
                        batches.push(lines);
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
        batches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn association_descriptor_is_complete_and_branded() {
        let entries = association_entries(&PathBuf::from("C:/Program Files/Player/player.exe"));
        assert!(entries[1].value.contains("player.exe"));
        assert!(entries[1].value.contains("%1"));
        assert_eq!(
            entries
                .iter()
                .filter(|e| e.key.starts_with("Software\\Classes\\."))
                .count(),
            SUPPORTED_EXTENSIONS.len()
        );
        assert!(entries.iter().all(|e| !e.value.contains("VLC")));
    }

    #[test]
    fn port_is_stable_and_in_range() {
        let port = instance_port();
        assert_eq!(port, instance_port());
        assert!((40_000..60_000).contains(&port));
    }

    #[test]
    fn second_instance_hand_off_round_trips() -> io::Result<()> {
        // Find a free port, release it, then compete for it deterministically.
        let probe = TcpListener::bind(("127.0.0.1", 0))?;
        let port = probe.local_addr()?.port();
        drop(probe);

        let first = SingleInstance::bind(port)?.expect("first instance acquires");
        assert!(SingleInstance::is_second_on(port));

        let media = vec!["a.mp4".to_owned(), "b.mkv".to_owned()];
        let notified = SingleInstance::notify_on(port, &media)?;
        assert!(notified);

        // accept() is non-blocking; a connection may need one more loop turn.
        let mut batches = Vec::new();
        for _ in 0..20 {
            batches = first.poll();
            if !batches.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(batches, vec![media]);
        drop(first);
        assert!(!SingleInstance::notify_on(port, &["x".to_owned()])?);
        Ok(())
    }
}
