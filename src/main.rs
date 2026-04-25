// ================================
// |   nbd server by Howard Chu   |
// ================================
//   o
//    o
//      o
//         /\      /\
//       -/--\----/--\-
//      /  /\      /\  \
//     /  (())    (())  \
//     \   \/      \/   /
//      \    (_/\_)    /
//       --------------

use std::net::{TcpListener, TcpStream};
use std::io::{Write, Read, SeekFrom, Seek};
use std::fs::{File, exists, OpenOptions};
use std::str::from_utf8;

mod s3;

// --- server URL
const URL: &str = "127.0.0.1:10809";

// --- NBD flags
const NBDMAGIC: u64 = 0x4e42444d41474943;
const IHAVEOPT: u64 = 0x49484156454F5054;
const NBD_FLAG_FIXED_NEWSTYLE: u16 = 1 << 0;
const NBD_REPLY_MAGIC: u64 = 0x3e889045565a9;
const NBD_REP_ACK: u32 = 1;
const NBD_REP_INFO: u32 = 3;
const BLOCK_SIZE: u64 = 50 * 1024 * 1024; // 20MB
const NBD_OPT_ABORT: u32 = 2;
const NBD_OPT_GO: u32 = 7;
const NBD_OPT_EXPORT_NAME: u32 = 1; // fallback
const NBD_INFO_EXPORT: u16 = 0;
const NBD_SIMPLE_REPLY_MAGIC: u32 = 0x67446698;

// --- transmission flags
const NBD_FLAG_SEND_FLUSH: u16 = 1 << 2;

// --- request types
const NBD_CMD_READ: u16 = 0;
const NBD_CMD_WRITE: u16 = 1;
const NBD_CMD_FLUSH: u16 = 3;

// default export
const EXPORT_DEFAULT: &str = "disk";

fn write_u16<W: Write>(writer: &mut W, data: u16) {
    writer.write_all(&data.to_be_bytes()).expect("failed to write u16");
}

fn write_u32<W: Write>(writer: &mut W, data: u32) {
    writer.write_all(&data.to_be_bytes()).expect("failed to write u32");
}

fn write_u64<W: Write>(writer: &mut W, data: u64) {
    writer.write_all(&data.to_be_bytes()).expect("failed to write u64");
}

fn read_u32<R: Read>(reader: &mut R) -> u32 {
    let mut buf: [u8; 4] = [0; 4];
    reader.read_exact(&mut buf).expect("failed to read u32");
    u32::from_be_bytes(buf)
}

fn read_u64<R: Read>(reader: &mut R) -> u64 {
    let mut buf: [u8; 8] = [0; 8];
    reader.read_exact(&mut buf).expect("failed to read u64");
    u64::from_be_bytes(buf)
}

#[allow(unused)]
fn clear_file(file: &mut File) {
    let buf = [0u8; 8192];
    let mut size: i64 = file.metadata().expect("fail to get metadata").len() as i64;
    println!("clearing file of size {}", size);
    while size > 0 {
        size -= file.write(&buf).expect("failed to write") as i64;
    }
    file.flush().expect("fail to flush");
}

// problem: it's not compact
#[derive(Default, Debug)]
struct ReqHeader {
    magic: u32,
    comm_flags: u16,
    type_: u16,
    cookie: u64,
    offset: u64,
    length: u32
}

fn read_header(stream: &mut TcpStream, req_header: &mut ReqHeader) {
    let mut buf = [0u8; 28];
    let _ = stream.read_exact(&mut buf).expect("failed to read request header");
    req_header.magic = u32::from_be_bytes(buf[0..4].try_into().expect("failed to turn slice to [u8; 4]"));
    req_header.comm_flags = u16::from_be_bytes(buf[4..6].try_into().expect("failed to turn slice to [u8; 2]"));
    req_header.type_ = u16::from_be_bytes(buf[6..8].try_into().expect("failed to turn slice to [u8; 2]"));
    req_header.cookie = u64::from_be_bytes(buf[8..16].try_into().expect("failed to turn slice to [u8; 8]"));
    req_header.offset = u64::from_be_bytes(buf[16..24].try_into().expect("failed to turn slice to [u8; 8]"));
    req_header.length = u32::from_be_bytes(buf[24..28].try_into().expect("failed to turn slice to [u8; 4]"));
}

fn reply(stream: &mut TcpStream, cookie: u64, buf: Option<&mut [u8]>) {
    write_u32(stream, NBD_SIMPLE_REPLY_MAGIC);
    write_u32(stream, 0); // error code
    write_u64(stream, cookie);
    if buf.is_some() {
        stream.write_all(buf.expect("I thought it was some")).expect("failed to write all");
    }
}

fn handle_traffic(mut stream: TcpStream) {
    // =========================== handshake ===========================
    write_u64(&mut stream, NBDMAGIC);
    write_u64(&mut stream, IHAVEOPT);
    let handshake_flags: u16 = NBD_FLAG_FIXED_NEWSTYLE;
    write_u16(&mut stream, handshake_flags);

    // client u32 flags
    let client_flags = read_u32(&mut stream);
    println!("client flags: {}", client_flags);
    if client_flags != NBD_FLAG_FIXED_NEWSTYLE as u32 {
        // SHOULD
        println!("client didn't set NBD_FLAG_FIXED_NEWSTYLE");
    }
    // client opts
    let client_i_have_opts = read_u64(&mut stream);
    println!("client_i_have_opts {}", client_i_have_opts);
    if client_i_have_opts != IHAVEOPT {
        // TODO: tell client it's unwelcome
        panic!("client didn't send IHAVEOPT");
    }
    let client_opt = read_u32(&mut stream);
    println!("client_opts {}", client_opt);
    let client_opt_len = read_u32(&mut stream);
    println!("client_opt_len {}", client_opt_len);
    let mut opt_buf: [u8; 2048] = [0; 2048];
    let _ = stream.read_exact(&mut opt_buf[..(client_opt_len as usize)]);
    let name_sz = u32::from_be_bytes(opt_buf[0..4].try_into().expect("slice to fixed"));
    let name_sz_uz = name_sz as usize;
    let mut export_name: String = String::from(EXPORT_DEFAULT);
    if name_sz != 0 {
        export_name = String::from(from_utf8(&opt_buf[4..4+name_sz_uz]).expect("failed to utf8"));
        println!("export name {}", export_name);
        let info_request_num = u16::from_be_bytes(opt_buf[4+name_sz_uz..6+name_sz_uz].try_into().expect("slice to fixed"));
        // we don't deal with them requests yet
        for i in 0..info_request_num as usize {
            let req = u16::from_be_bytes(opt_buf[6+name_sz_uz+i*2..6+name_sz_uz+(i+1)*2].try_into().expect("slice to fixed"));
            println!("info request number {}", req);
        }
    }

    if client_opt == NBD_OPT_ABORT {
        println!("client sent abort");
        write_u64(&mut stream, NBD_REPLY_MAGIC);
        write_u32(&mut stream, NBD_OPT_ABORT);
        write_u32(&mut stream, NBD_REP_ACK);
        write_u32(&mut stream, 0);
        println!("abort ACKed");
        return;
    }

    // we assume it's NBD_OPT_GO
    write_u64(&mut stream, NBD_REPLY_MAGIC);
    write_u32(&mut stream, NBD_OPT_EXPORT_NAME); // TODO: why NBD_OPT_EXPORT_NAME?
    write_u32(&mut stream, NBD_REP_INFO);
    write_u16(&mut stream, NBD_INFO_EXPORT);
    write_u16(&mut stream, 12);
    write_u16(&mut stream, NBD_INFO_EXPORT);
    write_u64(&mut stream, BLOCK_SIZE);
    write_u16(&mut stream, NBD_FLAG_SEND_FLUSH);// transmission flags
    println!("wrote size");

    write_u64(&mut stream, NBD_REPLY_MAGIC);
    write_u32(&mut stream, NBD_OPT_GO);
    write_u32(&mut stream, NBD_REP_ACK);
    write_u32(&mut stream, 0); // make it easy, set to 0
    println!("handshake completed");

    // =========================== transmission ===========================
    let export_file: String = export_name + ".img";
    if !exists(&export_file).expect("failed to check if exists") {
        File::create(&export_file).expect("failed to create file");
    }
    let mut file = OpenOptions::new().read(true).write(true).open(&export_file).expect("failed to open file");
    file.set_len(BLOCK_SIZE).expect("failed to set length");

    // only clear the file first time
    // clear_file(&mut file);

    let bucket = s3::S3Bucket::new();

    loop {
        file.rewind().expect("file failed to rewind");
        // read header
        let mut header = ReqHeader {..Default::default()};
        read_header(&mut stream, &mut header);
        println!("got request");
        println!("header {:?}", header);

        // TODO: implement read
        match header.type_ {
            NBD_CMD_READ => {
                file.seek(SeekFrom::Start(header.offset)).expect("failed to seek");
                let mut buf = vec![0u8; header.length as usize];
                let bytes = file.read(&mut buf[..]).expect("failed");
                println!("buffer size {}", buf[..].len());
                println!("read {} bytes from file", bytes);
                if bytes != header.length as usize {
                    println!("warning: bytes read ({}) != header.length ({})", bytes, header.length);
                }
                reply(&mut stream, header.cookie, Some(&mut buf[..]));
                println!("successfully read");
            },
            NBD_CMD_WRITE => {
                let mut buf = vec![0u8; header.length as usize];
                let mut to_read: isize = header.length as isize;
                let mut ptr = 0usize;
                while to_read > 0 {
                    let read = stream.read(&mut buf[ptr..]).expect("failed to read from stream") as isize;
                    to_read -= read;
                    ptr += read as usize;
                }
                file.seek(SeekFrom::Start(header.offset)).expect("failed to seek");
                let bytes = file.write(&mut buf[..]).expect("failed to write to file");
                println!("wrote {} bytes", bytes);
                reply(&mut stream, header.cookie, None);
                println!("successfully wrote");
            },
            NBD_CMD_FLUSH => {
                file.sync_all().expect("failed to fsync");
                bucket.put_object(&mut file, &export_file);
                reply(&mut stream, header.cookie, None);
                println!("successfully flushed");
            },
            _ => {
                reply(&mut stream, header.cookie, None);
                println!("successfully handled request type {}", header.type_);
            }
        }
    }
    
}

fn main() {
    println!("starting server");
    let listener = TcpListener::bind(URL).expect("failed to open listener");
    for _stream in listener.incoming() {
        println!("got stream");
        let stream = _stream.expect("no stream");
        handle_traffic(stream);
    }
    // transmission
}