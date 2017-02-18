//! A minimal implementation of Adler32 for Rust.
//!
//! This provides the simple method adler32(), that exhausts a Read and
//! computes the Adler32 hash, as well as the RollingAdler32 struct, that can
//! build a hash byte-by-byte, allowing to 'forget' past bytes in a rolling
//! fashion.
//!
//! The adler32 code has been translated (as accurately as I could manage) from
//! the zlib implementation.

#[cfg(test)]
extern crate rand;

use std::io;

// adler32 algorithm and implementation taken from zlib; http://www.zlib.net/
// It was translated into Rust as accurately as I could manage
// The (slow) reference was taken from Wikipedia; https://en.wikipedia.org/

/* zlib.h -- interface of the 'zlib' general purpose compression library
  version 1.2.8, April 28th, 2013

  Copyright (C) 1995-2013 Jean-loup Gailly and Mark Adler

  This software is provided 'as-is', without any express or implied
  warranty.  In no event will the authors be held liable for any damages
  arising from the use of this software.

  Permission is granted to anyone to use this software for any purpose,
  including commercial applications, and to alter it and redistribute it
  freely, subject to the following restrictions:

  1. The origin of this software must not be misrepresented; you must not
     claim that you wrote the original software. If you use this software
     in a product, an acknowledgment in the product documentation would be
     appreciated but is not required.
  2. Altered source versions must be plainly marked as such, and must not be
     misrepresented as being the original software.
  3. This notice may not be removed or altered from any source distribution.

  Jean-loup Gailly        Mark Adler
  jloup@gzip.org          madler@alumni.caltech.edu

*/

// largest prime smaller than 65536
const BASE: u32 = 65521;

// NMAX is the largest n such that 255n(n+1)/2 + (n+1)(BASE-1) <= 2^32-1
const NMAX: usize = 5552;

#[inline(always)]
fn do1(adler: &mut u32, sum2: &mut u32, buf: &[u8]) {
    *adler += buf[0] as u32;
    *sum2 += *adler;
}

#[inline(always)]
fn do2(adler: &mut u32, sum2: &mut u32, buf: &[u8]) {
    do1(adler, sum2, &buf[0..1]);
    do1(adler, sum2, &buf[1..2]);
}

#[inline(always)]
fn do4(adler: &mut u32, sum2: &mut u32, buf: &[u8]) {
    do2(adler, sum2, &buf[0..2]);
    do2(adler, sum2, &buf[2..4]);
}

#[inline(always)]
fn do8(adler: &mut u32, sum2: &mut u32, buf: &[u8]) {
    do4(adler, sum2, &buf[0..4]);
    do4(adler, sum2, &buf[4..8]);
}

#[inline(always)]
fn do16(adler: &mut u32, sum2: &mut u32, buf: &[u8]) {
    do8(adler, sum2, &buf[0..8]);
    do8(adler, sum2, &buf[8..16]);
}

/// A rolling version of the Adler32 hash, which can 'forget' past bytes.
///
/// Calling remove() will update the hash to the value it would have if that
/// past byte had never been fed to the algorithm. This allows you to get the
/// hash of a rolling window very efficiently.
pub struct RollingAdler32 {
    a: u32,
    b: u32,
}

impl RollingAdler32 {
    /// Creates an empty Adler32 context (with hash 1).
    pub fn new() -> RollingAdler32 {
        Self::from_value(1)
    }

    /// Creates an Adler32 context with the given initial value.
    pub fn from_value(adler32: u32) -> RollingAdler32 {
        let a = adler32 & 0xFFFF;
        let b = adler32 >> 16;
        RollingAdler32 { a: a, b: b }
    }

    /// Convenience function initializing a context from the hash of a buffer.
    pub fn from_buffer(buffer: &[u8]) -> RollingAdler32 {
        let mut hash = RollingAdler32::new();
        hash.update_buffer(buffer);
        hash
    }

    /// Returns the current hash.
    pub fn hash(&self) -> u32 {
        (self.b << 16) | self.a
    }

    /// Removes the given `byte` that was fed to the algorithm `size` bytes ago.
    pub fn remove(&mut self, size: usize, byte: u8) {
        let byte = byte as u32;
        self.a = (self.a + BASE - byte) % BASE;
        self.b = (self.b + BASE - 1 + (BASE - size as u32) * byte) % BASE;
    }

    /// Feeds a new `byte` to the algorithm to update the hash.
    pub fn update(&mut self, byte: u8) {
        let byte = byte as u32;
        self.a = (self.a + byte) % BASE;
        self.b = (self.b + self.a) % BASE;
    }

    /// Feeds a vector of bytes to the algorithm to update the hash.
    pub fn update_buffer(&mut self, buffer: &[u8]) {
        let len = buffer.len();

        // in case user likes doing a byte at a time, keep it fast
        if len == 1 {
            self.update(buffer[0]);
            return;
        }

        // in case short lengths are provided, keep it somewhat fast
        if len < 16 {
            for pos in 0..len {
                self.a += buffer[pos] as u32;
                self.b += self.a;
            }
            if self.a >= BASE {
                self.a -= BASE;
            }
            self.b %= BASE;
            return;
        }

        let mut pos = 0;

        // do length NMAX blocks -- requires just one modulo operation;
        while pos + NMAX <= len {
            let end = pos + NMAX;
            while pos < end {
                // 16 sums unrolled
                do16(&mut self.a, &mut self.b, &buffer[pos..pos + 16]);
                pos += 16;
            }
            self.a %= BASE;
            self.b %= BASE;
        }

        // do remaining bytes (less than NMAX, still just one modulo)
        if pos < len { // avoid modulos if none remaining
            while len - pos >= 16 {
                do16(&mut self.a, &mut self.b, &buffer[pos..pos + 16]);
                pos += 16;
            }
            while len - pos > 0 {
                self.a += buffer[pos] as u32;
                self.b += self.a;
                pos += 1;
            }
            self.a %= BASE;
            self.b %= BASE;
        }
    }
}

pub struct FixedSizeAdler32 {
    adler32: RollingAdler32,
    buffer: Box<[u8]>,
    position: usize,
    size: usize,
}

impl FixedSizeAdler32 {
    /// Creates an empty Adler32 context with a buffer of the given size.
    pub fn new(size: usize) -> FixedSizeAdler32 {
        Self::from_value(size, 1)
    }

    /// Creates an empty Adler32 context with the given size and value.
    pub fn from_value(size: usize, adler32: u32) -> FixedSizeAdler32 {
        FixedSizeAdler32 {
            adler32: RollingAdler32::from_value(adler32),
            buffer: vec![0; size].into_boxed_slice(),
            position: 0,
            size: 0,
        }
    }

    /// Creates an Adler32 context with an explicit buffer.
    pub fn with_buffer(adler32: u32, buffer: Box<[u8]>,
                       position: usize) -> FixedSizeAdler32 {
        FixedSizeAdler32 {
            adler32: RollingAdler32::from_value(adler32),
            buffer: buffer,
            position: position,
            size: position,
        }
    }

    pub fn hash(&self) -> u32 {
        self.adler32.hash()
    }
}

impl io::Write for FixedSizeAdler32 {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Keep total size, which will be returned
        let total_size = buf.len();
        let buffer_size = self.buffer.len();

        // Truncate the given array to the size of our internal buffer
        let size = if total_size > buffer_size {
            buffer_size
        } else {
            total_size
        };
        let buf = &buf[(total_size - size)..];
        println!("size = {}", size);

        // Remove old bytes
        let start = buffer_size + self.position - self.size;
        let remove = if self.size + size > buffer_size {
            self.size + size - buffer_size
        } else {
            0
        };
        println!("remove = {}", remove);
        for i in 0..remove {
            println!("Removing [{}] = '{}' ({})", (start + i) % buffer_size,
                     self.buffer[(start + i) % buffer_size] as char,
                     self.size - i);
            self.adler32.remove(
                self.size - i,
                self.buffer[(start + i) % buffer_size]);
        }
        self.size -= remove;

        // Add new bytes
        println!("Adding \"{}\"", std::str::from_utf8(buf).unwrap());
        self.adler32.update_buffer(buf);
        self.size += size;
        if self.position + size > buffer_size {
            self.buffer[self.position..]
                .clone_from_slice(
                    &buf[..(buffer_size - self.position)]);
            self.buffer[..(size + self.position - buffer_size)]
                .clone_from_slice(
                    &buf[(buffer_size - self.position)..]);
        } else {
            self.buffer[self.position..(self.position + size)]
                .clone_from_slice(&buf);
        }
        self.position = (self.position + size) % buffer_size;
        println!("buffer = \"{}\"", std::str::from_utf8(&self.buffer).unwrap());
        println!("position = {}", self.position);
        println!();
        Ok(total_size)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.write(buf).map(|_| ())
    }
}

/// Consume a Read object and returns the Adler32 hash.
pub fn adler32<R: io::Read>(mut reader: R) -> io::Result<u32> {
    let mut hash = RollingAdler32::new();
    let mut buffer = [0u8; NMAX];
    let mut read = try!(reader.read(&mut buffer));
    while read > 0 {
        hash.update_buffer(&buffer[..read]);
        read = try!(reader.read(&mut buffer));
    }
    Ok(hash.hash())
}

#[cfg(test)]
mod test {
    use rand;
    use rand::Rng;
    use std::io;
    use std::io::Write;

    use super::{BASE, adler32, RollingAdler32, FixedSizeAdler32};

    fn adler32_slow<R: io::Read>(reader: R) -> io::Result<u32> {
        let mut a: u32 = 1;
        let mut b: u32 = 0;

        for byte in reader.bytes() {
            let byte = try!(byte) as u32;
            a = (a + byte) % BASE;
            b = (b + a) % BASE;
        }

        Ok((b << 16) | a)
    }

    #[test]
    fn testvectors() {
        fn do_test(v: u32, bytes: &[u8]) {
            let mut hash = RollingAdler32::new();
            hash.update_buffer(&bytes);
            assert_eq!(hash.hash(), v);

            let r = io::Cursor::new(bytes);
            assert_eq!(adler32(r).unwrap(), v);
        }
        do_test(0x00000001, b"");
        do_test(0x00620062, b"a");
        do_test(0x024d0127, b"abc");
        do_test(0x29750586, b"message digest");
        do_test(0x90860b20, b"abcdefghijklmnopqrstuvwxyz");
        do_test(0x8adb150c, b"ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                              abcdefghijklmnopqrstuvwxyz\
                              0123456789");
        do_test(0x97b61069, b"1234567890123456789012345678901234567890\
                              1234567890123456789012345678901234567890");
        do_test(0xD6251498, &[255; 64000]);
    }

    #[test]
    fn compare() {
        let mut rng = rand::thread_rng();
        let mut data = vec![0u8; 5589];
        for size in [0, 1, 3, 4, 5, 31, 32, 33, 67,
                     5550, 5552, 5553, 5568, 5584, 5589].iter().cloned() {
            rng.fill_bytes(&mut data[..size]);
            let r1 = io::Cursor::new(&data[..size]);
            let r2 = r1.clone();
            if adler32_slow(r1).unwrap() != adler32(r2).unwrap() {
                panic!("Comparison failed, size={}", size);
            }
        }
    }

    #[test]
    fn rolling() {
        assert_eq!(RollingAdler32::from_value(0x01020304).hash(), 0x01020304);

        fn do_test(a: &[u8], b: &[u8]) {
            let mut total = Vec::with_capacity(a.len() + b.len());
            total.extend(a);
            total.extend(b);
            let mut h = RollingAdler32::from_buffer(&total[..(b.len())]);
            for i in 0..(a.len()) {
                h.remove(b.len(), a[i]);
                h.update(total[b.len() + i]);
            }
            assert_eq!(h.hash(), adler32(b).unwrap());
        }
        do_test(b"a", b"b");
        do_test(b"", b"this a test");
        do_test(b"th", b"is a test");
        do_test(b"this a ", b"test");
    }

    #[test]
    fn fixed_size() {
        let mut rolling = FixedSizeAdler32::new(4);
        rolling.write(b"12").unwrap();
        assert_eq!(rolling.hash(), adler32(b"12" as &[u8]).unwrap());
        rolling.write(b"3").unwrap();
        assert_eq!(rolling.hash(), adler32(b"123" as &[u8]).unwrap());
        rolling.write(b"45").unwrap();
        assert_eq!(rolling.hash(), adler32(b"2345" as &[u8]).unwrap());
        rolling.write(b"67890123").unwrap();
        assert_eq!(rolling.hash(), adler32(b"0123" as &[u8]).unwrap());
    }
}
