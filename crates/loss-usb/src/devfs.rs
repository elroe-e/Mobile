//! usbdevfs access on Linux and Android: read the raw descriptors from a device file descriptor
//! (Android's `UsbDeviceConnection.getFileDescriptor()`) and send class control requests.
//! Control requests to an interface the kernel's audio driver holds can be refused (EBUSY);
//! callers report that rather than fail.

use crate::{Parsed, Queried};
use std::os::fd::RawFd;

#[repr(C)]
struct CtrlTransfer {
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    length: u16,
    timeout: u32,
    data: *mut libc::c_void,
}

/// `_IOWR('U', 0, struct usbdevfs_ctrltransfer)`.
const fn ioctl_control() -> u64 {
    let size = std::mem::size_of::<CtrlTransfer>() as u64;
    (3 << 30) | (size << 16) | ((b'U' as u64) << 8)
}

/// All descriptors (device, then configurations), as `read()` on usbdevfs returns them.
pub fn read_descriptors(fd: RawFd) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; 65536];
    let n = unsafe { libc::pread(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len(), 0) };
    if n < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    buf.truncate(n as usize);
    Ok(buf)
}

/// A class-specific IN request to an interface or entity; returns the bytes received.
fn get(fd: RawFd, request: u8, value: u16, index: u16, len: u16) -> Result<Vec<u8>, String> {
    let mut data = vec![0u8; len as usize];
    let mut t = CtrlTransfer {
        // Device to host, class, recipient interface.
        request_type: 0xA1,
        request,
        value,
        index,
        length: len,
        timeout: 1000,
        data: data.as_mut_ptr() as *mut libc::c_void,
    };
    let r = unsafe { libc::ioctl(fd, ioctl_control() as _, &mut t as *mut CtrlTransfer) };
    if r < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    data.truncate(r as usize);
    Ok(data)
}

fn db(v: &[u8], at: usize) -> f64 {
    i16::from_le_bytes([v[at], v[at + 1]]) as f64 / 256.0
}

const VOLUME: u16 = 0x02 << 8;
const SAM_FREQ: u16 = 0x01 << 8;

/// Asks for volume ranges and (UAC2) clock rates.
pub fn query(fd: RawFd, p: &Parsed) -> Queried {
    let mut q = Queried::default();
    let Some(ifc) = p.control_interface else {
        return q;
    };
    let uac2 = p.audio_version.is_some_and(|v| v >= 0x0200);
    for f in p.features.iter().filter(|f| f.volume) {
        let index = ((f.id as u16) << 8) | ifc as u16;
        let r = if uac2 {
            // RANGE: wNumSubRanges, then (min, max, res) triples; the overall span is enough.
            get(fd, 0x02, VOLUME, index, 2).and_then(|n| {
                let count = u16::from_le_bytes([n[0], *n.get(1).unwrap_or(&0)]).max(1);
                let v = get(fd, 0x02, VOLUME, index, 2 + 6 * count)?;
                if v.len() < 8 {
                    return Err("short answer".into());
                }
                let last = 2 + 6 * (count as usize - 1);
                Ok((db(&v, 2), db(&v, last + 2), db(&v, 6)))
            })
        } else {
            // GET_MIN / GET_MAX / GET_RES on the master channel.
            let one = |req| get(fd, req, VOLUME, index, 2).map(|v| db(&v, 0));
            one(0x82).and_then(|lo| Ok((lo, one(0x83)?, one(0x84)?)))
        };
        q.volume.push((f.id, r));
    }
    if uac2 {
        for &c in &p.clocks {
            let index = ((c as u16) << 8) | ifc as u16;
            let r = get(fd, 0x02, SAM_FREQ, index, 2).and_then(|n| {
                let count = u16::from_le_bytes([n[0], *n.get(1).unwrap_or(&0)]).max(1);
                let v = get(fd, 0x02, SAM_FREQ, index, 2 + 12 * count)?;
                let mut rates = Vec::new();
                for k in 0..count as usize {
                    let at = 2 + 12 * k;
                    let Some(x) = v.get(at..at + 12) else { break };
                    let u = |o: usize| u32::from_le_bytes([x[o], x[o + 1], x[o + 2], x[o + 3]]);
                    let (lo, hi, step) = (u(0), u(4), u(8));
                    if step == 0 || lo == hi {
                        rates.push(lo);
                    } else {
                        // A continuous range: list the common rates inside it.
                        for r in [44100, 48000, 88200, 96000, 176400, 192000, 352800, 384000] {
                            if lo <= r && r <= hi && (r - lo) % step == 0 {
                                rates.push(r);
                            }
                        }
                    }
                }
                Ok(rates)
            });
            q.clock_rates.push((c, r));
        }
    }
    q
}

#[cfg(test)]
mod tests {
    #[test]
    fn control_ioctl_number_matches_the_kernel() {
        // 0xC0185500 on 64-bit, 0xC0105500 on 32-bit (struct with a pointer at the end).
        let want = if cfg!(target_pointer_width = "64") {
            0xC018_5500
        } else {
            0xC010_5500
        };
        assert_eq!(super::ioctl_control(), want);
    }
}
