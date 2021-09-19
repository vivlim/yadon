use std::io::{Seek, SeekFrom, Write};


#[derive(Debug, Default)]
/// Stores write and seek operations to be replayed later.
pub struct Yadon {
    operations: Vec<WriteOperation>,
    virtual_position: Option<u64>,
    start: Option<u64>,
    length: Option<u64>,
}

#[derive(Debug)]
/// Write / Seek operations which were called on Yadon
pub enum WriteOperation {
    Write(Vec<u8>),
    Seek(SeekFrom)
}

impl Yadon {
    pub fn new(start: Option<u64>, length: Option<u64>) -> Self {
        Yadon {
            operations: vec![],
            virtual_position: None,
            start,
            length,
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

impl Write for Yadon {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.operations.push(WriteOperation::Write(buf.into()));

        self.virtual_position = match self.virtual_position {
            Some(current_position) => Some(current_position + buf.len() as u64),
            None => Some(buf.len() as u64)
        };

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Seek for Yadon {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.operations.push(WriteOperation::Seek(pos));

        match (self.virtual_position, pos, self.start, self.length) {
            (_, SeekFrom::Start(from_start), _, _) => {
                self.virtual_position = Some(from_start);
            }
            (None, SeekFrom::Current(from_current), Some(start_position), _) => {
                self.virtual_position = Some((start_position as i64 + from_current) as u64);
            }
            (_, SeekFrom::End(from_end), _, Some(length)) => {
                self.virtual_position = Some((length as i64 + from_end) as u64); // We can't know what the end position of the final writer will actually be ... so just say that we're at the 0th position :|
            }
            (Some(current_pos), SeekFrom::Current(from_current), _, _) => {
                self.virtual_position = Some((current_pos as i64 + from_current) as u64)
            }
            (_, SeekFrom::End(_), _, None) => {
                self.virtual_position = None;
            }
            (None, SeekFrom::Current(_), None, _) => {
                self.virtual_position = None;
            }
        }

        match self.virtual_position {
            Some(pos) => Ok(pos),
            None => Err(std::io::ErrorKind::Unsupported.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Seek, SeekFrom, Write};
    use crate::Yadon;

    #[test]
    fn delayed_write() {
        let mut later = Yadon::new(Some(0), Some(16));
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

    #[test]
    fn return_values() {
        let mut now_target = vec![0u8; 128];
        let mut now = Cursor::new(&mut now_target);
        now.seek(SeekFrom::Start(27));
        let mut later = Yadon::new(Some(27), Some(128));

        assert_multi_seek(&mut now, &mut later, SeekFrom::Current(0));
        assert_multi_seek(&mut now, &mut later, SeekFrom::Current(4));
        assert_multi_write(&mut now, &mut later, &[1,2,3,4,5]);
        assert_multi_seek(&mut now, &mut later, SeekFrom::Current(-2));
        assert_multi_write(&mut now, &mut later, &[1,2,3,4,5]);
        assert_multi_seek(&mut now, &mut later, SeekFrom::Start(27));
        assert_multi_seek(&mut now, &mut later, SeekFrom::Current(2));
        assert_multi_write(&mut now, &mut later, &[1,2]);
        assert_multi_seek(&mut now, &mut later, SeekFrom::End(-12));
        assert_multi_write(&mut now, &mut later, &[12; 14]);
    }

    fn assert_multi_write<T1, T2>(a: &mut T1, b: &mut T2, buf: &[u8])
    where T1: Write + Seek, T2: Write + Seek {
        let result1 = a.write(buf);
        let result2 = b.write(buf);

        match (result1, result2) {
            (Ok(a_bytes), Ok(b_bytes)) => {
                println!("{}, {} written", a_bytes, b_bytes);
                assert_eq!(a_bytes, b_bytes)
            },
            (a_res, b_res) => {
                assert!(false, "results differ: {:?} and {:?}", a_res, b_res);
            }
        }
    }

    fn assert_multi_seek<T1, T2>(a: &mut T1, b: &mut T2, pos: SeekFrom)
    where T1: Write + Seek, T2: Write + Seek {
        let result1 = a.seek(pos);
        let result2 = b.seek(pos);

        match (result1, result2) {
            (Ok(a_pos), Ok(b_pos)) => {
                println!("{}, {} seeked", a_pos, b_pos);
                assert_eq!(a_pos, b_pos)
            },
            (a_res, b_res) => {
                assert!(false, "results differ: {:?} and {:?}", a_res, b_res);
            }
        }

    }
}