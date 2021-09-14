use std::io::{Seek, SeekFrom, Write};


#[derive(Debug, Default)]
/// Stores write and seek operations to be replayed later.
pub struct BidingWriter {
    operations: Vec<WriteOperation>,
    virtual_position: Option<u64>,
}

#[derive(Debug)]
/// Write / Seek operations which were called on the BidingWriter
pub enum WriteOperation {
    Write(Vec<u8>),
    Seek(SeekFrom)
}

impl BidingWriter {
    pub fn new() -> Self {
        BidingWriter {
            operations: vec![],
            virtual_position: None,
        }
    }

    /// Applies the stored operations on a target writer. Operations are not consumed.
    pub fn apply<T>(&self, target: &mut T) -> std::io::Result<usize> where T: Write + Seek {
        let mut bytes_written: usize = 0;
        for operation in &self.operations {
            match operation {
                WriteOperation::Write(data) => {
                    bytes_written += target.write(&data)?;
                },
                WriteOperation::Seek(pos) => {
                    target.seek(*pos)?;
                }
            }
        }
        target.flush()?;
        Ok(bytes_written)
    }
}

impl Write for BidingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.operations.push(WriteOperation::Write(buf.into()));

        self.virtual_position = match self.virtual_position {
            Some(current_position) => Some(current_position + buf.len() as u64),
            None => Some(buf.len() as u64)
        };

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        todo!()
    }
}

impl Seek for BidingWriter {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.operations.push(WriteOperation::Seek(pos));

        match (self.virtual_position, pos) {
            (_, SeekFrom::Start(from_start)) => {
                self.virtual_position = Some(from_start);
            }
            (None, SeekFrom::Current(from_current)) => {
                if from_current > 0 {
                    self.virtual_position = Some(from_current as u64);
                }
                else {
                    self.virtual_position = Some(0); // We can't know what the final end position is, so just say we're at 0
                }
            }
            (_, SeekFrom::End(_)) => {
                self.virtual_position = Some(0); // We can't know what the end position of the final writer will actually be ... so just say that we're at the 0th position :|
            }
            (Some(current_pos), SeekFrom::Current(from_current)) => {
                self.virtual_position = Some((current_pos as i64 + from_current) as u64)
            }
        }

        match self.virtual_position {
            Some(pos) => Ok(pos),
            None => Err(std::io::ErrorKind::Unsupported.into()),
        }
        // Result 
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Seek, SeekFrom, Write};
    use crate::BidingWriter;

    #[test]
    fn delayed_write() {
        let mut later = BidingWriter::new();
        later.seek(SeekFrom::Start(4)).unwrap();
        later.write(&[1,2,3]).unwrap();
        later.seek(SeekFrom::End(-2)).unwrap();
        later.write(&[4,5]).unwrap();
        later.seek(SeekFrom::Current(-6)).unwrap();
        later.write(&[6,7,8]).unwrap();

        let mut target = vec![0u8; 16];
        let mut target_writer = Cursor::new(&mut target);
        later.apply(&mut target_writer).unwrap();
        assert_eq!(target, &[0, 0, 0, 0, 1, 2, 3, 0, 0, 0, 6, 7, 8, 0, 4, 5]);
    }
}
