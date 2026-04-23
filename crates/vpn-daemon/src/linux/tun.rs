use std::io::{self};
use std::os::{fd::AsRawFd, fd::FromRawFd, fd::OwnedFd};
pub struct TunFd(pub OwnedFd);
use nix::errno::Errno;
use nix::unistd;
use std::ffi::{CStr, CString};

impl AsRawFd for TunFd {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        self.0.as_raw_fd()
    }
}

#[allow(unused)]
pub struct TunInterface {
    fd: tokio::io::unix::AsyncFd<TunFd>,
    name: String,
}

fn errno_to_io(err: Errno) -> io::Error {
    io::Error::from_raw_os_error(err as i32)
}
impl TunInterface {
    pub fn new(fd: OwnedFd, name: String) -> io::Result<Self> {
        Ok(Self {
            fd: tokio::io::unix::AsyncFd::new(TunFd(fd))?,
            name: name,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    //pub fn fd(&self) -> std::sync::Arc<tokio::io::unix::AsyncFd<TunFd>> {
    //    self.fd.clone()
    //}
    pub async fn read_packet(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let mut guard = self.fd.readable().await?;
            match guard.try_io(|s| match unistd::read(&s.get_ref().0, buf) {
                Ok(result) => Ok(result),
                Err(Errno::EAGAIN) => Err(io::Error::from(io::ErrorKind::WouldBlock)),
                Err(err) => Err(errno_to_io(err)),
            }) {
                Ok(result) => {
                    let n = result?;
                    return Ok(n);
                }
                Err(_would_block) => continue,
            }
        }
    }
    pub async fn write_packet(&self, packet: &mut [u8]) -> io::Result<usize> {
        loop {
            let mut guard = self.fd.writable().await?;

            match guard.try_io(|inner| match unistd::write(&inner.get_ref().0, packet) {
                Ok(result) if result == packet.len() => return Ok(result),
                Ok(_) => Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "too short packet to write",
                )),
                Err(Errno::EAGAIN) => Err(io::Error::from(io::ErrorKind::WouldBlock)),
                Err(err) => Err(errno_to_io(err)),
            }) {
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }
}

pub fn create_interface(name: &str) -> io::Result<(OwnedFd, String)> {
    if name.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "interface name must not be empty",
        )
        .into());
    }
    let path = std::ffi::CString::new("/dev/net/tun")
        .expect("static path must not containt interior NULL");
    let ptr = path.as_ptr();
    let raw_file_d = unsafe { libc::open(ptr, libc::O_RDWR | libc::O_NONBLOCK) };
    if raw_file_d < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut ifr: libc::ifreq = unsafe { std::mem::zeroed() };
    let name_c = CString::new(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "interface name contains interior NUL byte",
        )
    })?;
    let name_bytes = name_c.as_bytes_with_nul();

    if name_bytes.len() > libc::IFNAMSIZ {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "interface name too long: max {} bytes including trailing NUL",
                libc::IFNAMSIZ
            ),
        ));
    }
    unsafe {
        std::ptr::copy_nonoverlapping(name_c.as_ptr(), ifr.ifr_name.as_mut_ptr(), name_bytes.len());

        ifr.ifr_ifru.ifru_flags = (libc::IFF_TUN | libc::IFF_NO_PI) as libc::c_short;
    }

    let ioctl = unsafe { libc::ioctl(raw_file_d, libc::TUNSETIFF, &ifr) };
    if ioctl < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let actual_name = unsafe { CStr::from_ptr(ifr.ifr_name.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    let owned = unsafe { OwnedFd::from_raw_fd(raw_file_d) };

    Ok((owned, actual_name))
}
