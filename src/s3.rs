use s3::bucket::Bucket;
use awsregion::Region;
use s3::creds::Credentials;
use std::io::{Read, Write};

const BUCKET: &str = "howard";
const URL: &str = "http://192.168.86.40:9000";
const REGION: &str = "us-east-1";

pub struct S3Bucket {
    bucket: Box<Bucket>
}

impl S3Bucket {
    pub fn new() -> Self {
        let credentials = Credentials::new(Some("minioadmin"), Some("minioadmin"), None, None, None).expect("failed to create credentials");
        let region = Region::Custom {
            region: REGION.to_string(),
            endpoint: URL.to_string()
        };
        let bucket = Bucket::new(BUCKET, region, credentials).expect("failed to create bucket");
        S3Bucket {
            bucket: bucket
        }
    }

    pub fn get_object<W: Write + std::marker::Send>(&self, mut writable: W, filename: &str) {
        let response = self.bucket.with_path_style().get_object_to_writer(filename, &mut writable).expect("foo");
        println!("get object response {:?}", response);
    }

    pub fn put_object<R: Read>(&self, mut readble: R, filename: &str) {
        let response = self.bucket.with_path_style().put_object_stream(&mut readble, filename).expect("failed to save to s3");
        println!("put object response {:?}", response);
    }

    pub fn object_exists(&self, filename: &str) -> bool {
        self.bucket.with_path_style().object_exists(filename).expect("failed to check if object exists")
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_s3() {
        let bucket = S3Bucket::new();
        let exist = bucket.bucket.with_path_style().exists().expect("fail to get existence");
        println!("exists {}", exist);
    }

    #[test]
    fn test_put_object() {
        let bucket = S3Bucket::new();
        let cursor = Cursor::new([1, 2, 3]);
        bucket.put_object(cursor, "onetwothree.txt");
    }

    #[test]
    fn test_object_exists() {
        let bucket = S3Bucket::new();
        // should always exists...
        let object_name = "test_device.img";
        assert_eq!(bucket.object_exists(object_name), true);
    }
}