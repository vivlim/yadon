use thiserror::Error;
use std::io::{Seek, SeekFrom, Write};
use std::fmt::Debug;

#[derive(Debug, Default)]
/// Stores write and seek operations to be replayed later.
/// # Example
/// ```
/// use yadon::Yadon;
/// use std::io::{Cursor, Write, Seek, SeekFrom};
/// let mut target = vec![0u8; 8];
/// let mut yadon = Yadon::new(Some(0), Some(target.len() as u64));
/// assert_eq!(yadon.seek(SeekFrom::Start(4)).unwrap(), 4);
/// assert_eq!(yadon.write(&[1,2,3]).unwrap(), 3);
/// assert_eq!(yadon.seek(SeekFrom::Current(0)).unwrap(), 7);
/// assert_eq!(yadon.seek(SeekFrom::End(-6)).unwrap(), 2);
/// assert_eq!(yadon.write(&[4,5]).unwrap(), 2);
/// assert_eq!(yadon.seek(SeekFrom::Current(0)).unwrap(), 4);
/// 
/// let mut target_writer = Cursor::new(&mut target);
/// // Apply the stored operations to our target writer.
/// // Pass `true` so the return values of the seeks and writes are compared with the simulated values.
/// yadon.apply(&mut target_writer, true).unwrap();
/// assert_eq!(target, &[0, 0, 4, 5, 1, 2, 3, 0]);
/// ```
/// # Remarks
/// * If a start position is set when apply() is called, the target will seek to the start position.
/// * Boundary checks are only performed during `write()` if length is specified.
pub struct Yadon {
    /// Stored operations
    pub operations: Vec<WriteOperation>,
    /// Virtual position to use for emulating the return values of another Write + Seek
    virtual_position: Option<u64>,
    /// If set, used to set the initial virtual cursor position. `apply()` will seek to this position before applying.
    pub start: Option<u64>,
    /// If set, used to emulate cursor position for SeekFrom::End operations. If not set, seeks involving SeekFrom::End will fail, returning `Err(std::io::ErrorKind::Unsupported)`
    pub length: Option<u64>,
}

/// Errors that may occur while applying `Yadon`.
#[derive(Error, Debug)]
pub enum ApplyError {
    /// IO error while trying to replay operations.
    #[error("io error while trying to replay operations")]
    Io(#[from] std::io::Error),
    /// Seek position diverged while trying to replay operations.
    #[error("seek position diverged while trying to replay operations")]
    SeekDiverged(Confusion<u64>),
    /// Number of bytes written diverged while trying to replay operations.
    #[error("number of bytes written diverged while trying to replay operations")]
    NumBytesWrittenDiverge(Confusion<usize>)
}

/// During apply, there was divergence between the expected return value of an operation, and its result.
#[derive(Debug)]
pub struct Confusion<T>
where T: Debug {
    /// The value which we returned when the operation was first simulated.
    pub expected: T,
    /// The value which was returned when trying to apply this operation to another Write + Seek.
    pub actual: T,
}

#[derive(Debug)]
/// Write + Seek operations which were called on Yadon
pub enum WriteOperation {
    /// Write something, and check that the number of bytes written matches.
    Write(Vec<u8>, usize),
    /// Seek somewhere, and check that the resulting position matches.
    Seek(SeekFrom, u64)
}

impl Yadon {
    /// Constructs an instance of `Yadon` with optional `start` position and `length`, which, if set, should match
    /// whatever you plan to apply `Yadon` to later.
    pub fn new(start: Option<u64>, length: Option<u64>) -> Self {
        Yadon {
            operations: vec![],
            virtual_position: None,
            start,
            length,
        }
    }

    /// Applies the stored operations on a target writer. Operations are not consumed, and may be replayed again.
    /// If a `start` position was specified, this will seek to that position before applying.
    /// If `check_return_values` is set, the result of each seek / write will be compared to the
    /// simulated return value, and the apply will fail if it is different.
    pub fn apply<T>(&self, target: &mut T, check_return_values: bool) -> Result<usize, ApplyError> where T: Write + Seek {
        if let Some(start) = self.start {
            let seek_pos = target.seek(SeekFrom::Start(start))?;
            if check_return_values && seek_pos != start {
                // Something is wrong with the seek.
                return Err(ApplyError::SeekDiverged(Confusion {
                    expected: start,
                    actual: seek_pos
                }));
            }
        }
        let mut total_bytes_written: usize = 0;
        for operation in &self.operations {
            match operation {
                WriteOperation::Write(data, expected_bytes_written) => {
                    let bytes_written = target.write(&data)?;
                    if check_return_values && *expected_bytes_written != bytes_written {
                        return Err(ApplyError::NumBytesWrittenDiverge(Confusion{
                            expected: *expected_bytes_written,
                            actual: bytes_written
                        }));
                    }
                    total_bytes_written += bytes_written;
                },
                WriteOperation::Seek(pos, expected_position) => {
                    let new_position = target.seek(*pos)?;
                    if check_return_values && new_position != *expected_position {
                        return Err(ApplyError::SeekDiverged(Confusion{
                            expected: *expected_position,
                            actual: new_position
                        }));
                    }
                }
            }
        }
        target.flush()?;
        Ok(total_bytes_written)
    }
}

impl Write for Yadon {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let (None, Some(start), Some(_)) = (self.virtual_position, self.start, self.length) {
            // If the start position is specified and this is the first operation, and we're doing length
            // emulation, the virtual position must be initialized.
            self.virtual_position = Some(start);
        }

        let buf = match self.length {
            Some(max_length) => { // Emulate writing into something with a max length
                let available_space = match self.virtual_position {
                    Some(current_position) => (max_length - current_position) as usize,
                    None => max_length as usize
                };

                if buf.len() > available_space {
                    &buf[0..available_space]
                } else {
                    buf
                }
            },
            None => {
                buf
            }
        };


        self.virtual_position = match self.virtual_position {
            Some(current_position) => Some(current_position + buf.len() as u64),
            None => Some(buf.len() as u64)
        };

        self.operations.push(WriteOperation::Write(buf.into(), buf.len()));
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Seek for Yadon {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {

        match (self.virtual_position, pos, self.start, self.length) {
            (_, SeekFrom::Start(from_start), _, _) => {
                self.virtual_position = Some(from_start);
            }
            (None, SeekFrom::Current(from_current), Some(start_position), _) => {
                self.virtual_position = Some((start_position as i64 + from_current) as u64);
            }
            (_, SeekFrom::End(from_end), _, Some(length)) => {
                self.virtual_position = Some((length as i64 + from_end) as u64);
            }
            (Some(current_pos), SeekFrom::Current(from_current), _, _) => {
                self.virtual_position = Some((current_pos as i64 + from_current) as u64)
            }
            (_, SeekFrom::End(_), _, None) => {
                self.virtual_position = None; // This will return an ErrorKind::Unsupported.
            }
            (None, SeekFrom::Current(from_current), None, _) => {
                self.virtual_position = Some(from_current as u64); // If a start waas not specified, assume we're at position 0.
            }
        }

        match self.virtual_position {
            Some(resulting_position) => {
                self.operations.push(WriteOperation::Seek(pos, resulting_position));
                Ok(resulting_position)
            },
            None => Err(std::io::ErrorKind::Unsupported.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Seek, SeekFrom, Write};
    use crate::{ApplyError, Yadon};

    #[test]
    fn delayed_write() {
        let mut yadon = Yadon::new(Some(0), Some(16));
        assert_eq!(yadon.seek(SeekFrom::Start(4)).unwrap(), 4);
        assert_eq!(yadon.write(&[1,2,3]).unwrap(), 3);
        assert_eq!(yadon.seek(SeekFrom::End(-2)).unwrap(), 14);
        assert_eq!(yadon.write(&[4,5]).unwrap(), 2);
        assert_eq!(yadon.seek(SeekFrom::Current(-6)).unwrap(), 10);
        assert_eq!(yadon.write(&[6,7,8]).unwrap(), 3);

        let mut target = vec![0u8; 16];
        let mut target_writer = Cursor::new(&mut target);
        yadon.apply(&mut target_writer, true).unwrap();
        assert_eq!(target, &[0, 0, 0, 0, 1, 2, 3, 0, 0, 0, 6, 7, 8, 0, 4, 5]);
    }

    #[test]
    fn start_and_end() {
        let mut yadon = Yadon::new(Some(1), Some(4));
        assert_eq!(yadon.seek(SeekFrom::Current(2)).unwrap(), 3);
        yadon.write(&[1]).unwrap();
        assert_eq!(yadon.seek(SeekFrom::End(-3)).unwrap(), 1);
        yadon.write(&[2]).unwrap();

        let mut target = vec![0u8; 4];
        let mut target_writer = Cursor::new(&mut target);
        yadon.apply(&mut target_writer, true).unwrap();
        assert_eq!(target, &[0, 2, 0, 1]);
    }

    #[test]
    fn unspecified_length_end_seek_fails() {
        let mut yadon = Yadon::new(None, None);
        assert_eq!(yadon.seek(SeekFrom::End(-3)).map_err(|e| e.kind()), Err(std::io::ErrorKind::Unsupported.into()));
    }

    #[test]
    fn unspecified_start_current_seek_assumes_0() {
        let mut yadon = Yadon::new(None, None);
        assert_eq!(yadon.seek(SeekFrom::Current(2)).unwrap(), 2);
    }

    #[test]
    fn too_big_write() {
        let mut now_target = [0u8; 32];
        let mut now = Cursor::new(&mut now_target[..]);
        now.seek(SeekFrom::Start(1)).unwrap();
        let mut yadon = Yadon::new(Some(1), Some(32));
        // Too big since we start at position 1
        assert_eq!(assert_multi_write(&mut now, &mut yadon, &[1u8; 64]).unwrap(), 31);
        assert_multi_seek(&mut now, &mut yadon, SeekFrom::End(-16)).unwrap();
        assert_eq!(assert_multi_write(&mut now, &mut yadon, &[2u8; 17]).unwrap(), 16);
        assert_multi_seek(&mut now, &mut yadon, SeekFrom::Start(31)).unwrap();
        assert_eq!(assert_multi_write(&mut now, &mut yadon, &[3u8; 2]).unwrap(), 1);
        assert_eq!(assert_multi_write(&mut now, &mut yadon, &[4u8; 2]).unwrap(), 0);
        assert_multi_seek(&mut now, &mut yadon, SeekFrom::End(0)).unwrap();
        assert_eq!(assert_multi_write(&mut now, &mut yadon, &[5u8; 2]).unwrap(), 0);
        assert_multi_write(&mut now, &mut yadon, &[1]).unwrap();
        assert_multi_write(&mut now, &mut yadon, &[1]).unwrap();

        println!("{:?}", yadon);
        let mut later_target = vec![0u8; 32]; // Using a vec here because it can grow if we've made a mistake
        let mut later_writer = Cursor::new(&mut later_target[..]);
        yadon.apply(&mut later_writer, true).unwrap();
        assert_eq!(&later_target, &now_target);
    }

    #[test]
    fn return_values() {
        let mut now_target = [0u8; 128];
        let mut now = Cursor::new(&mut now_target[..]);
        now.seek(SeekFrom::Start(27)).unwrap();
        let mut yadon = Yadon::new(Some(27), Some(128));

        assert_multi_seek(&mut now, &mut yadon, SeekFrom::Current(0)).unwrap();
        assert_multi_seek(&mut now, &mut yadon, SeekFrom::Current(4)).unwrap();
        assert_multi_write(&mut now, &mut yadon, &[1,2,3,4,5]).unwrap();
        assert_multi_seek(&mut now, &mut yadon, SeekFrom::Current(-2)).unwrap();
        assert_multi_write(&mut now, &mut yadon, &[1,2,3,4,5]).unwrap();
        assert_multi_seek(&mut now, &mut yadon, SeekFrom::Start(27)).unwrap();
        assert_multi_seek(&mut now, &mut yadon, SeekFrom::Current(2)).unwrap();
        assert_multi_write(&mut now, &mut yadon, &[1,2]).unwrap();
        assert_multi_seek(&mut now, &mut yadon, SeekFrom::End(-12)).unwrap();
        assert_multi_write(&mut now, &mut yadon, &[12; 14]).unwrap();

        let mut later_target = vec![0u8; 128]; // Using a vec here because it can grow if we've made a mistake
        let mut later_writer = Cursor::new(&mut later_target[..]);
        yadon.apply(&mut later_writer, true).unwrap();
        assert_eq!(&later_target, &now_target);
    }

    #[test]
    fn failed_apply_write_end() {
        // mismatched sizes between yadon and the target
        let mut yadon = Yadon::new(Some(1), Some(4));
        assert_eq!(yadon.seek(SeekFrom::End(-3)).unwrap(), 1);
        yadon.write(&[2]).unwrap();

        let mut target = vec![0u8; 8];
        let mut target_writer = Cursor::new(&mut target);
        // we should be able to successfully apply this if check_return_values is false.
        yadon.apply(&mut target_writer, false).unwrap();

        // but if it's true, we should get an error
        match yadon.apply(&mut target_writer, true) {
            Err(ApplyError::SeekDiverged(diff)) => {
                assert_eq!(diff.expected, 1);
                assert_eq!(diff.actual, 5);
            },
            res => {
                assert!(false, "Apply did not fail with a diverged seek: {:?}", res);
            }
        }
    }

    #[test]
    fn apply_smaller_than_target() {
        let mut yadon = Yadon::new(Some(1), Some(4));
        yadon.write(&[0; 6]).unwrap();

        let mut target = vec![0u8; 8];
        let mut target_writer = Cursor::new(&mut target);
        assert_eq!(yadon.apply(&mut target_writer, true).unwrap(), 3);
    }

    #[test]
    fn failed_apply_write_too_much() {
        // mismatched sizes between yadon and the target
        let mut yadon = Yadon::new(Some(1), Some(8));
        yadon.write(&[0; 6]).unwrap();

        let mut target = [0u8; 4];
        let mut target_writer = Cursor::new(&mut target[..]);

        // we should be able to successfully write this if check_return_values is false.
        yadon.apply(&mut target_writer, false).unwrap();

        // but if it's true, we should get an error
        match yadon.apply(&mut target_writer, true) {
            Err(ApplyError::NumBytesWrittenDiverge(diff)) => {
                assert_eq!(diff.expected, 6);
                assert_eq!(diff.actual, 3);
            },
            res => {
                assert!(false, "Apply did not fail with a diverged write: {:?}", res);
            }
        }
    }
    
    #[test]
    fn cannot_seek_end_without_length() {
        // mismatched sizes between yadon and the target
        let mut yadon = Yadon::new(Some(3), None);
        yadon.write(&[0; 6]).unwrap();
        assert_eq!(yadon.seek(SeekFrom::End(-2)).map_err(|e| e.kind()), Err(std::io::ErrorKind::Unsupported.into()));
    }

    fn assert_multi_write<T1, T2>(a: &mut T1, b: &mut T2, buf: &[u8]) -> std::io::Result<usize>
    where T1: Write + Seek, T2: Write + Seek {
        let result1 = a.write(buf);
        let result2 = b.write(buf);

        match (result1, result2) {
            (Ok(a_bytes), Ok(b_bytes)) => {
                println!("{}, {} written", a_bytes, b_bytes);
                assert_eq!(a_bytes, b_bytes);
                Ok(a_bytes)
            },
            (a_res, b_res) => {
                assert!(false, "results differ: {:?} and {:?}", a_res, b_res);
                a_res
            }
        }
    }

    fn assert_multi_seek<T1, T2>(a: &mut T1, b: &mut T2, pos: SeekFrom) -> std::io::Result<u64>
    where T1: Write + Seek, T2: Write + Seek {
        let result1 = a.seek(pos);
        let result2 = b.seek(pos);

        match (result1, result2) {
            (Ok(a_pos), Ok(b_pos)) => {
                println!("{}, {} seeked", a_pos, b_pos);
                assert_eq!(a_pos, b_pos);
                Ok(a_pos)
            },
            (a_res, b_res) => {
                assert!(false, "results differ: {:?} and {:?}", a_res, b_res);
                a_res
            }
        }
    }
}