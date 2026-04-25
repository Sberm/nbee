use std::fs::{File, OpenOptions, exists};
use std::io::{SeekFrom, Read, Write, Seek};

use crate::s3::S3Bucket;
use crate::BLOCK_SIZE;

pub struct FileHandler {
    filename: String,
    file: File, /* temporary file that acts as a layer of cache on top of s3 */
    bucket: S3Bucket,
    is_copy: bool,
    copied_from: Option<String>
}

fn clear_file(file: &mut File) {
    let buf = [0u8; 8192];
    let mut size: i64 = file.metadata().expect("fail to get metadata").len() as i64;
    println!("clearing file of size {}", size);
    while size > 0 {
        size -= file.write(&buf).expect("failed to write") as i64;
    }
    file.flush().expect("fail to flush");
}

fn get_tmp_file(filename: &str) -> File {
    if !exists(filename).expect("failed to check if exists") {
        File::create(filename).expect("failed to create file");
        let mut file = OpenOptions::new().read(true).write(true).open(filename).expect("failed to open file");
        file.set_len(BLOCK_SIZE).expect("failed to set length");
        clear_file(&mut file);
    }
    OpenOptions::new().read(true).write(true).open(filename).expect("failed to open file")
}

impl FileHandler {
    /// input should be "disk", with "_forks" and ".img" uffix removed
    pub fn fork(from_filename: &str) -> Self {
        FileHandler {
            filename: from_filename.to_string() + "_fork.img",
            file: get_tmp_file(&(from_filename.to_string() + ".img")),
            bucket: S3Bucket::new(),
            is_copy: true,
            copied_from: Some(from_filename.to_string())
        }
    }

    pub fn new(filename: &str) -> Self {
        FileHandler {
            filename: filename.to_string() + ".img",
            file: get_tmp_file(&(filename.to_string() + ".img")),
            bucket: S3Bucket::new(),
            is_copy: false,
            copied_from: None
        }
    }

    pub fn rewind(&mut self) {
        self.file.rewind().expect("failed to rewind");
    }

    pub fn read(&mut self, offset: u64, buf: &mut [u8]) -> usize {
        self.file.seek(SeekFrom::Start(offset)).expect("failed to seek");
        self.file.read(buf).expect("failed to read")
    }

    pub fn write(&mut self, offset: u64, buf: &mut [u8]) -> usize {
        self.file.seek(SeekFrom::Start(offset)).expect("failed to seek");
        self.file.write(buf).expect("failed to write")
    }

    pub fn flush(&mut self) {
        self.file.sync_all().expect("failed to sync");
        self.bucket.put_object(&mut self.file, &self.filename);
    }
}