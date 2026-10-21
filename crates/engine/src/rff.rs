//! Reader for RFF ("Resource File Format") archives used by Blood.
//!
//! Layout (all little endian):
//! * header, 32 bytes: magic `RFF\x1a`, u16 version, u16 pad, u32 dictionary offset,
//!   u32 entry count, 16 bytes padding.
//! * dictionary: `count` entries of 48 bytes, XOR-obfuscated for versions >= 0x300.
//! * entries flagged [`FLAG_ENCRYPTED`] have their first 256 bytes XOR-obfuscated.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

pub const FLAG_BY_ID: u8 = 0x01;
pub const FLAG_EXTERNAL: u8 = 0x02;
pub const FLAG_PRELOAD: u8 = 0x04;
pub const FLAG_PRELOCK: u8 = 0x08;
pub const FLAG_ENCRYPTED: u8 = 0x10;

const ENTRY_SIZE: usize = 48;

#[derive(Debug, Clone)]
pub struct RffEntry {
    pub offset: u32,
    pub size: u32,
    pub time: u32,
    pub flags: u8,
    /// Upper-case extension without dot, e.g. `MAP`.
    pub ext: String,
    /// Upper-case base name, e.g. `E1M1`.
    pub name: String,
    pub id: u32,
}

impl RffEntry {
    pub fn file_name(&self) -> String {
        format!("{}.{}", self.name, self.ext)
    }
}

pub struct Rff {
    data: Vec<u8>,
    pub version: u16,
    pub entries: Vec<RffEntry>,
    by_name: HashMap<String, usize>,
    by_id: HashMap<(String, u32), usize>,
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

fn cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_ascii_uppercase()
}

/// XOR obfuscation used for file payloads and v0x301 dictionaries:
/// byte `i` is XORed with `(key + i) >> 1`.
pub fn crypt(buf: &mut [u8], key: u32) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b ^= (key.wrapping_add(i as u32) >> 1) as u8;
    }
}

/// Dictionary obfuscation of v0x300 archives: byte `i` XOR `((key + i) & 0xff) >> 1`.
fn crypt_v300(buf: &mut [u8], key: u32) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b ^= ((key.wrapping_add(i as u32) & 0xff) >> 1) as u8;
    }
}

impl Rff {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        Self::from_bytes(fs::read(path)?)
    }

    pub fn from_bytes(data: Vec<u8>) -> io::Result<Self> {
        if data.len() < 32 || &data[0..4] != b"RFF\x1a" {
            return Err(invalid("not an RFF archive"));
        }
        let version = u16::from_le_bytes([data[4], data[5]]);
        let dict_off = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
        let count = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
        let dict_len = count * ENTRY_SIZE;
        if dict_off + dict_len > data.len() {
            return Err(invalid("RFF dictionary out of range"));
        }
        let mut dict = data[dict_off..dict_off + dict_len].to_vec();
        let key0 = (dict_off & 0xff) as u32;
        match version {
            0x0200 => {}
            0x0300 => crypt_v300(&mut dict, key0),
            0x0301 => crypt(&mut dict, key0 * 2),
            _ => return Err(invalid(format!("unsupported RFF version {version:#x}"))),
        }

        let mut entries = Vec::with_capacity(count);
        let mut by_name = HashMap::new();
        let mut by_id = HashMap::new();
        for (i, e) in dict.as_chunks::<ENTRY_SIZE>().0.iter().enumerate() {
            let rd = |o: usize| u32::from_le_bytes(e[o..o + 4].try_into().unwrap());
            let entry = RffEntry {
                offset: rd(16),
                size: rd(20),
                time: rd(28),
                flags: e[32],
                ext: cstr(&e[33..36]),
                name: cstr(&e[36..44]),
                id: rd(44),
            };
            by_name.insert(entry.file_name(), i);
            by_id.insert((entry.ext.clone(), entry.id), i);
            entries.push(entry);
        }
        Ok(Rff {
            data,
            version,
            entries,
            by_name,
            by_id,
        })
    }

    pub fn find(&self, file_name: &str) -> Option<&RffEntry> {
        self.by_name
            .get(&file_name.to_ascii_uppercase())
            .map(|&i| &self.entries[i])
    }

    pub fn find_id(&self, ext: &str, id: u32) -> Option<&RffEntry> {
        self.by_id
            .get(&(ext.to_ascii_uppercase(), id))
            .map(|&i| &self.entries[i])
    }

    /// Returns the decrypted contents of an entry.
    pub fn read(&self, entry: &RffEntry) -> io::Result<Vec<u8>> {
        let start = entry.offset as usize;
        let end = start + entry.size as usize;
        if end > self.data.len() {
            return Err(invalid(format!("entry {} out of range", entry.file_name())));
        }
        let mut out = self.data[start..end].to_vec();
        if entry.flags & FLAG_ENCRYPTED != 0 {
            let n = out.len().min(256);
            crypt(&mut out[..n], 0);
        }
        Ok(out)
    }

    pub fn read_name(&self, file_name: &str) -> Option<Vec<u8>> {
        self.find(file_name).and_then(|e| self.read(e).ok())
    }

    pub fn read_id(&self, ext: &str, id: u32) -> Option<Vec<u8>> {
        self.find_id(ext, id).and_then(|e| self.read(e).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crypt_roundtrip() {
        let orig: Vec<u8> = (0..300u32).map(|i| (i * 7) as u8).collect();
        let mut b = orig.clone();
        crypt(&mut b, 17);
        assert_ne!(b, orig);
        crypt(&mut b, 17);
        assert_eq!(b, orig);
    }

    #[test]
    fn parse_synthetic_v301() {
        let payload: Vec<u8> = b"HELLO BLOOD".to_vec();
        let mut file = vec![0u8; 32];
        file[0..4].copy_from_slice(b"RFF\x1a");
        file[4..6].copy_from_slice(&0x0301u16.to_le_bytes());
        let data_off = file.len() as u32;
        let mut enc = payload.clone();
        crypt(&mut enc, 0);
        file.extend_from_slice(&enc);
        let dict_off = file.len() as u32;
        let mut e = vec![0u8; 48];
        e[16..20].copy_from_slice(&data_off.to_le_bytes());
        e[20..24].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        e[32] = FLAG_ENCRYPTED;
        e[33..36].copy_from_slice(b"TXT");
        e[36..40].copy_from_slice(b"TEST");
        e[44..48].copy_from_slice(&5u32.to_le_bytes());
        crypt(&mut e, (dict_off & 0xff) * 2);
        file.extend_from_slice(&e);
        file[8..12].copy_from_slice(&dict_off.to_le_bytes());
        file[12..16].copy_from_slice(&1u32.to_le_bytes());

        let rff = Rff::from_bytes(file).unwrap();
        assert_eq!(rff.entries[0].file_name(), "TEST.TXT");
        assert_eq!(rff.read_name("test.txt").unwrap(), payload);
        assert_eq!(rff.read_id("TXT", 5).unwrap(), payload);
    }
}
