extern crate alloc;

use alloc::vec::Vec;
use core::default::Default;

/// This is the equivalent to the
/// [SeekFrom](https://doc.rust-lang.org/std/io/enum.SeekFrom.html) type from
/// the rust standard library, but reproduced here to avoid dependency on
/// `std::io`.
pub enum SeekFrom {
    Start(u64),
    End(i64),
    Current(i64),
}

/// The `Seek` trait provides a cursor which can be moved within a stream of
/// bytes. This is essentially a copy of
/// [std::io::Seek](https://doc.rust-lang.org/std/io/trait.Seek.html), but
/// avoiding its dependency on `std::io::Error`, and the associated code size
/// increase.
pub trait Seek {
    type Err;
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Err>;
}

/// The `Read` trait provides a means of reading from byte streams.
pub trait Read {
    type Err: Default;
    /// Read a number of bytes into the provided buffer. The returned value is
    /// `Ok(n)` if a read was successful, and `n` bytes were read (`n` could be
    /// 0).
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Err>;

    /// Read exactly the required number of bytes. If not enough bytes could be
    /// read the function returns `Err(_)`, and the contents of the given buffer
    /// is unspecified.
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), Self::Err> {
        let mut start = 0;
        while start < buf.len() {
            match self.read(&mut buf[start..]) {
                Ok(0) => break,
                Ok(n) => {
                    start += n;
                }
                Err(_e) => return Err(Default::default()),
            }
        }
        if start == buf.len() {
            Ok(())
        } else {
            Err(Default::default())
        }
    }

    /// Read a `u32` in little-endian format.
    fn read_u64(&mut self) -> Result<u64, Self::Err> {
        let mut bytes = [0u8; 8];
        self.read_exact(&mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }

    /// Read a `u32` in little-endian format.
    fn read_u32(&mut self) -> Result<u32, Self::Err> {
        let mut bytes = [0u8; 4];
        self.read_exact(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    /// Read a `u8`.
    fn read_u8(&mut self) -> Result<u8, Self::Err> {
        let mut bytes = [0u8; 1];
        self.read_exact(&mut bytes)?;
        Ok(bytes[0])
    }
}

/// The `Write` trait provides functionality for writing to byte streams.
pub trait Write {
    type Err: Default;
    /// Try to write the given buffer into the output stream. If writes are
    /// successful returns the number of bytes written.
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Err>;

    /// Attempt to write the entirety of the buffer to the output by repeatedly
    /// calling `write` until either no more output can written, or writing
    /// fails.
    fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Err> {
        let mut start = 0;
        while start < buf.len() {
            match self.write(&buf[start..]) {
                Ok(n) if n > 0 => start += n,
                _ => return Err(Default::default()),
            }
        }
        Ok(())
    }

    /// Write a single byte to the output.
    fn write_u8(&mut self, x: u8) -> Result<(), Self::Err> { self.write_all(&x.to_le_bytes()) }

    /// Write a `u16` in little endian.
    fn write_u16(&mut self, x: u16) -> Result<(), Self::Err> { self.write_all(&x.to_le_bytes()) }

    /// Write a `u32` in little endian.
    fn write_u32(&mut self, x: u32) -> Result<(), Self::Err> { self.write_all(&x.to_le_bytes()) }

    /// Write a `u64` in little endian.
    fn write_u64(&mut self, x: u64) -> Result<(), Self::Err> { self.write_all(&x.to_le_bytes()) }
}

impl Write for Vec<u8> {
    type Err = ();

    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Err> {
        let _ = self.extend_from_slice(buf);
        Ok(buf.len())
    }
}

/// The `Serialize` trait provides a means of writing structures into byte-sinks
/// (`Write`) or reading structures from byte sources (`Read`).
pub trait Serialize: Sized {
    /// Attempt to write the structure into the provided writer, failing if
    /// only part of the structure could be written.
    fn serial<W: Write>(&self, _out: &mut W) -> Option<()>;
    /// Attempt to read a structure from a given source, failing if an error
    /// occurs during deserialization or reading.
    fn deserial<R: Read>(_source: &mut R) -> Option<Self>;
}

pub trait Get<T> {
    fn get(&mut self) -> Option<T>;
}

impl<R: Read, T: Serialize> Get<T> for R {
    #[inline(always)]
    fn get(&mut self) -> Option<T> { T::deserial(self) }
}
