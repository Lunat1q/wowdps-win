//! The local transport every client and the daemon speak over, named by a
//! path ([`crate::client::socket_path`], the overlay's claim).
//!
//! Unix: a unix socket at the path; the 0700 socket dir keeps other users
//! out. Windows: std has no unix sockets there, so the listener is loopback
//! TCP on a port the OS picks, and the path is a file holding
//! `<port> <token>`. A client sends the token before anything else and the
//! listener drops a connection that does not, so only a reader of that file
//! (the per-user app data dir) gets in, as only the owner of the socket dir
//! does on unix.

use std::io;
use std::path::Path;

#[cfg(unix)]
pub use std::os::unix::net::{UnixListener as Listener, UnixStream as Stream};

/// Connect to the listener at `path`.
pub fn connect(path: impl AsRef<Path>) -> io::Result<Stream> {
    #[cfg(unix)]
    return Stream::connect(path);
    #[cfg(windows)]
    return windows::connect(path.as_ref());
}

/// Listen at `path`. The caller removes a stale path first.
pub fn bind(path: impl AsRef<Path>) -> io::Result<Listener> {
    #[cfg(unix)]
    return Listener::bind(path);
    #[cfg(windows)]
    return windows::bind(path.as_ref());
}

/// Two connected ends, no path (tests, in-process bridges).
pub fn pair() -> io::Result<(Stream, Stream)> {
    #[cfg(unix)]
    return Stream::pair();
    #[cfg(windows)]
    return windows::pair();
}

#[cfg(windows)]
pub use windows::{Listener, Stream};

#[cfg(windows)]
mod windows {
    use std::hash::{BuildHasher, Hasher, RandomState};
    use std::io::{self, Read, Write};
    use std::net::{Ipv4Addr, TcpListener, TcpStream};
    use std::path::Path;
    use std::time::Duration;

    pub type Stream = TcpStream;

    /// Hex digits in a token.
    const TOKEN_LEN: usize = 32;

    /// How long an accepted connection has to present its token.
    // ponytail: checked on the accept thread, so a local process that
    // connects and stays silent stalls other connects this long; move the
    // check to the connection's thread if that ever matters.
    const ADMIT_TIMEOUT: Duration = Duration::from_secs(2);

    pub struct Listener {
        inner: TcpListener,
        token: String,
    }

    impl std::fmt::Debug for Listener {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Listener")
                .field("inner", &self.inner)
                .finish_non_exhaustive()
        }
    }

    impl Listener {
        /// The next connection that presents the token.
        pub fn accept(&self) -> io::Result<(Stream, std::net::SocketAddr)> {
            loop {
                let (mut stream, addr) = self.inner.accept()?;
                if self.admit(&mut stream) {
                    let _ = stream.set_nodelay(true);
                    return Ok((stream, addr));
                }
            }
        }

        pub fn incoming(&self) -> impl Iterator<Item = io::Result<Stream>> + '_ {
            std::iter::repeat_with(|| self.accept().map(|(s, _)| s))
        }

        fn admit(&self, stream: &mut Stream) -> bool {
            let mut got = [0u8; TOKEN_LEN];
            stream.set_read_timeout(Some(ADMIT_TIMEOUT)).is_ok()
                && stream.read_exact(&mut got).is_ok()
                && got == *self.token.as_bytes()
                && stream.set_read_timeout(None).is_ok()
        }
    }

    pub fn connect(path: &Path) -> io::Result<Stream> {
        let text = std::fs::read_to_string(path)?;
        let (port, token) = text
            .trim()
            .split_once(' ')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad address file"))?;
        let port: u16 = port
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad port"))?;
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))?;
        stream.write_all(token.as_bytes())?;
        let _ = stream.set_nodelay(true);
        Ok(stream)
    }

    pub fn bind(path: &Path) -> io::Result<Listener> {
        let inner = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let token = token();
        let port = inner.local_addr()?.port();
        std::fs::write(path, format!("{port} {token}"))?;
        Ok(Listener { inner, token })
    }

    pub fn pair() -> io::Result<(Stream, Stream)> {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let a = TcpStream::connect(l.local_addr()?)?;
        let (b, _) = l.accept()?;
        let _ = (a.set_nodelay(true), b.set_nodelay(true));
        Ok((a, b))
    }

    /// 128 unguessable bits: std's hasher keys come from the OS's random
    /// source, and its output under a secret key is unpredictable.
    fn token() -> String {
        (0..2u64)
            .map(|i| {
                let mut h = RandomState::new().build_hasher();
                h.write_u64(i);
                format!("{:016x}", h.finish())
            })
            .collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn only_the_token_gets_in() {
            let dir = std::env::temp_dir().join(format!("wowdps-sock-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("t.sock");
            let listener = bind(&path).unwrap();
            let port = listener.inner.local_addr().unwrap().port();
            // A stranger without the token is dropped, the client gets in.
            let t = std::thread::spawn(move || {
                let mut s = listener.accept().unwrap().0;
                let mut b = [0u8; 2];
                s.read_exact(&mut b).unwrap();
                b
            });
            let mut stranger = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            stranger.write_all(&[b'x'; TOKEN_LEN]).unwrap();
            let mut client = connect(&path).unwrap();
            client.write_all(b"ok").unwrap();
            assert_eq!(&t.join().unwrap(), b"ok");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
