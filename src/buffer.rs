use smallvec::SmallVec;
use std::cmp;
use tokio::io::{self, AsyncRead, AsyncReadExt};

/// Optimized ring buffer that gives slightly better performance in this use case than Tokio's BufReader.
pub struct RingBufReader<R: AsyncRead + Unpin, const N: usize> {
    reader: R,
    buf: [u8; N],
    pos: usize,
    len: usize,
}

impl<R: AsyncRead + Unpin, const N: usize> RingBufReader<R, N> {
    pub fn new(reader: R) -> RingBufReader<R, N> {
        RingBufReader {
            reader,
            buf: [0; N],
            pos: 0,
            len: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    async fn fill_buffer(&mut self) -> io::Result<()> {
        let start = (self.pos + self.len) % N;
        let end = cmp::min(N, start + (N - self.len));
        let nread = self.reader.read(&mut self.buf[start..end]).await?;
        if nread == 0 {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "Disconnected",
            ));
        }
        self.len += nread;
        Ok(())
    }

    pub async fn read_u8(&mut self) -> io::Result<u8> {
        while self.len == 0 {
            self.fill_buffer().await?;
        }
        let last_pos = self.pos;
        self.pos = (self.pos + 1) % N;
        self.len -= 1;
        Ok(self.buf[last_pos])
    }

    pub async fn read_exact(
        &mut self,
        target: &mut SmallVec<[u8; 16]>,
        length: usize,
    ) -> io::Result<()> {
        loop {
            let cur_pos = self.pos;
            let bytes_to_copy = cmp::min(length - target.len(), cmp::min(self.len, N - cur_pos));
            if bytes_to_copy == 0 {
                break;
            }
            self.pos = (self.pos + bytes_to_copy) % N;
            self.len -= bytes_to_copy;
            // println!("Copying from {}..{}", cur_pos, (cur_pos + bytes_to_copy));
            // println!("pos = {} len = {}", self.pos, self.len);
            target.extend_from_slice(&mut self.buf[cur_pos..(cur_pos + bytes_to_copy)]);
        }

        if target.len() != length {
            let start = target.len();
            target.resize(length, 0);
            self.reader.read_exact(&mut target[start..length]).await?;
        }

        Ok(())
    }
}
