use std::fs::{File, OpenOptions, exists, copy};
use std::io::{SeekFrom, Read, Write, Seek};

use crate::s3::S3Bucket;
use crate::BLOCK_SIZE;

pub struct FileHandler {
    filename: String,
    file: File, /* temporary file that acts as a layer of cache on top of s3 */
    bucket: S3Bucket,
    is_copy: bool,
    copied_from: Option<String>,
    pend_write: bool
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

fn get_file_or_empty(filename: &str, bucket: &S3Bucket) -> File {
    if !exists(filename).expect("failed to check if exists") {
        File::create(filename).expect("failed to create file");
        let mut file = OpenOptions::new().read(true).write(true).open(filename).expect("failed to open file");
        
        if bucket.object_exists(filename) {
            println!("S3 object found for {}", filename);
            bucket.get_object(file, filename);
        } else {
            // if you can't pull from s3, just create an empty file
            println!("Can't find S3 object, creating empty file");
            file.set_len(BLOCK_SIZE).expect("failed to set length");
            clear_file(&mut file);
        }
    }
    OpenOptions::new().read(true).write(true).open(filename).expect("failed to open file")
}

/// TODO: pull the file from s3 if temp file doesn't exist
impl FileHandler {
    /// input should be "disk", with "_forks" and ".img" suffixes removed
    pub fn fork(from_filename: &str) -> Self {
        println!("forking new file");
        let bucket = S3Bucket::new();
        FileHandler {
            filename: from_filename.to_string() + "_fork.img",
            file: get_file_or_empty(&(from_filename.to_string() + ".img"), &bucket),
            bucket: bucket,
            is_copy: true,
            copied_from: Some(from_filename.to_string() + ".img"),
            pend_write: false
        }
    }

    pub fn new(filename: &str) -> Self {
        println!("creating new file");
        let bucket = S3Bucket::new();
        FileHandler {
            filename: filename.to_string() + ".img",
            file: get_file_or_empty(&(filename.to_string() + ".img"), &bucket),
            bucket: bucket,
            is_copy: false,
            copied_from: None,
            pend_write: false
        }
    }

    pub fn read(&mut self, offset: u64, buf: &mut [u8]) -> usize {
        self.file.seek(SeekFrom::Start(offset)).expect("failed to seek");
        self.file.read(buf).expect("failed to read")
    }

    pub fn write(&mut self, offset: u64, buf: &mut [u8]) -> usize {
        // triggers copy-on-write
        if self.is_copy {
            println!("write triggers copy-on-write");
            let copied_from_ref = self.copied_from.as_ref();
            println!("copying from {} to {}", copied_from_ref.expect("copied_from failed"), &self.filename);
            let copied = copy(copied_from_ref.expect("failed to unwrap copied_from"), &self.filename).expect("failed to copy");
            println!("copied {} bytes", copied);
            self.file = get_file_or_empty(&self.filename, &self.bucket);
            self.is_copy = false;
            self.copied_from = None;
            // copy the s3 object too
            self.bucket.put_object(&mut self.file, &self.filename);
        }
        self.pend_write = true;
        self.file.seek(SeekFrom::Start(offset)).expect("failed to seek");
        self.file.write(buf).expect("failed to write")
    }

    pub fn flush(&mut self) {
        if !self.pend_write {
            return;
        }
        if self.is_copy {
            println!("flush triggers copy-on-write");
            let copied_from_ref = self.copied_from.as_ref();
            println!("copying from {} to {}", copied_from_ref.expect("copied_from failed"), &self.filename);
            let copied = copy(copied_from_ref.expect("failed to unwrap copied_from"), &self.filename).expect("failed to copy");
            println!("copied {} bytes", copied);
            // wait till it's copied
            self.file.sync_all().expect("failed to sync");
            self.file = get_file_or_empty(&self.filename, &self.bucket);
            self.is_copy = false;
            self.copied_from = None;
        }
        self.pend_write = false;
        self.file.sync_all().expect("failed to sync");
        self.bucket.put_object(&mut self.file, &self.filename);
    }
}