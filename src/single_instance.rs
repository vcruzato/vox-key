use std::io;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener};

/// Binds an abstract unix socket unique to `key`. Returns `AddrInUse` if another
/// voxkey instance already holds it. Keep the returned listener alive for the
/// process lifetime; dropping it releases the lock.
pub fn acquire(key: &str) -> io::Result<UnixListener> {
    let name = format!("voxkey-{key}");
    let addr = SocketAddr::from_abstract_name(name.as_bytes())?;
    UnixListener::bind_addr(&addr)
}
