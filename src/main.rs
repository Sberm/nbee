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

use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use tokio::time::{sleep, Duration};
use tokio::sync::Mutex;
use std::str::from_utf8;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

mod s3;
mod file;

use crate::file::file::FileHandler;

// --- server URL
const URL: &str = "127.0.0.1:10809";

// --- NBD flags
const NBDMAGIC: u64 = 0x4e42444d41474943;
const IHAVEOPT: u64 = 0x49484156454F5054;
const NBD_FLAG_FIXED_NEWSTYLE: u16 = 1 << 0;
const NBD_REPLY_MAGIC: u64 = 0x3e889045565a9;
const NBD_REP_ACK: u32 = 1;
const NBD_REP_INFO: u32 = 3;
const BLOCK_SIZE: u64 = 5 * 1024 * 1024; // 5MB
const NBD_OPT_ABORT: u32 = 2;
const NBD_OPT_GO: u32 = 7;
const NBD_OPT_EXPORT_NAME: u32 = 1; // fallback
const NBD_INFO_EXPORT: u16 = 0;
const NBD_SIMPLE_REPLY_MAGIC: u32 = 0x67446698;

// --- transmission flags
const NBD_FLAG_SEND_FLUSH: u16 = 1 << 2;
const NBD_FLAG_SEND_FUA: u16 = 1 << 3;

const NBD_CMD_FLAG_FUA: u16 = 1 << 0;

// --- request types
const NBD_CMD_READ: u16 = 0;
const NBD_CMD_WRITE: u16 = 1;
const NBD_CMD_DISC: u16 = 2;
const NBD_CMD_FLUSH: u16 = 3;

// --- default export
const EXPORT_DEFAULT: &str = "disk";

// --- export suffix
const EXPORT_SUFFIX: &str = "_fork";

// IOPS = TOKEN_MX / SLEEP_TIME * MSEC_PER_SEC(1000)
const SLEEP_TIME: u64 = 200;
const TOKEN_REFILL_TIME: u64 = SLEEP_TIME;
const TOKEN_MX: usize = 100;

async fn write_u16<W: AsyncWriteExt + Unpin>(writer: &mut W, data: u16) {
    match writer.write_all(&data.to_be_bytes()).await {
        Ok(_) => {},
        Err(e) => {eprintln!("{}", e)}
    }
}

async fn write_u32<W: AsyncWriteExt + Unpin>(writer: &mut W, data: u32) {
    match writer.write_all(&data.to_be_bytes()).await {
        Ok(_) => {},
        Err(e) => {eprintln!("{}", e)}
    }
}

async fn write_u64<W: AsyncWriteExt + Unpin>(writer: &mut W, data: u64) {
    match writer.write_all(&data.to_be_bytes()).await {
        Ok(_) => {},
        Err(e) => {eprintln!("{}", e)}
    }
}

async fn read_u32<R: AsyncReadExt + Unpin>(reader: &mut R) -> u32 {
    let mut buf: [u8; 4] = [0; 4];
    reader.read_exact(&mut buf).await.expect("failed to read u32");
    u32::from_be_bytes(buf)
}

async fn read_u64<R: AsyncReadExt + Unpin>(reader: &mut R) -> u64 {
    let mut buf: [u8; 8] = [0; 8];
    reader.read_exact(&mut buf).await.expect("failed to read u64");
    u64::from_be_bytes(buf)
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

async fn read_header(stream: &mut TcpStream, req_header: &mut ReqHeader) {
    let mut buf = [0u8; 28];
    let _ = stream.read_exact(&mut buf).await.expect("failed to read request header");
    req_header.magic = u32::from_be_bytes(buf[0..4].try_into().expect("failed to turn slice to [u8; 4]"));
    req_header.comm_flags = u16::from_be_bytes(buf[4..6].try_into().expect("failed to turn slice to [u8; 2]"));
    req_header.type_ = u16::from_be_bytes(buf[6..8].try_into().expect("failed to turn slice to [u8; 2]"));
    req_header.cookie = u64::from_be_bytes(buf[8..16].try_into().expect("failed to turn slice to [u8; 8]"));
    req_header.offset = u64::from_be_bytes(buf[16..24].try_into().expect("failed to turn slice to [u8; 8]"));
    req_header.length = u32::from_be_bytes(buf[24..28].try_into().expect("failed to turn slice to [u8; 4]"));
}

async fn reply(stream: &mut TcpStream, cookie: u64, buf: Option<&mut [u8]>) {
    write_u32(stream, NBD_SIMPLE_REPLY_MAGIC).await;
    write_u32(stream, 0).await; // error code
    write_u64(stream, cookie).await;
    if buf.is_some() {
        stream.write_all(buf.expect("I thought it was some")).await.expect("failed to write all");
    }
}

async fn handle_traffic(mut stream: TcpStream, token_bucket: Arc<AtomicUsize>) {
    println!("got stream");

    // =========================== handshake ===========================
    write_u64(&mut stream, NBDMAGIC).await;
    write_u64(&mut stream, IHAVEOPT).await;
    let handshake_flags: u16 = NBD_FLAG_FIXED_NEWSTYLE;
    write_u16(&mut stream, handshake_flags).await;

    // client u32 flags
    let client_flags = read_u32(&mut stream).await;
    println!("client flags: {}", client_flags);
    // TODO: that conversion looks weird
    if client_flags != NBD_FLAG_FIXED_NEWSTYLE as u32 {
        // SHOULD
        println!("client didn't set NBD_FLAG_FIXED_NEWSTYLE");
    }
    // client opts
    let client_i_have_opts = read_u64(&mut stream).await;
    println!("client_i_have_opts {}", client_i_have_opts);
    if client_i_have_opts != IHAVEOPT {
        // TODO: tell client it's unwelcome
        panic!("client didn't send IHAVEOPT");
    }
    let client_opt = read_u32(&mut stream).await;
    println!("client_opts {}", client_opt);
    let client_opt_len = read_u32(&mut stream).await;
    println!("client_opt_len {}", client_opt_len);
    let mut opt_buf: [u8; 2048] = [0; 2048];
    let _ = stream.read_exact(&mut opt_buf[..(client_opt_len as usize)]).await;
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
        write_u64(&mut stream, NBD_REPLY_MAGIC).await;
        write_u32(&mut stream, NBD_OPT_ABORT).await;
        write_u32(&mut stream, NBD_REP_ACK).await;
        write_u32(&mut stream, 0).await;
        println!("abort ACKed");
        return;
    }

    // we assume it's NBD_OPT_GO
    write_u64(&mut stream, NBD_REPLY_MAGIC).await;
    write_u32(&mut stream, NBD_OPT_EXPORT_NAME).await; // TODO: why NBD_OPT_EXPORT_NAME?
    write_u32(&mut stream, NBD_REP_INFO).await;
    write_u16(&mut stream, NBD_INFO_EXPORT).await;
    write_u16(&mut stream, 12).await;
    write_u16(&mut stream, NBD_INFO_EXPORT).await;
    write_u64(&mut stream, BLOCK_SIZE).await;
    write_u16(&mut stream, NBD_FLAG_SEND_FLUSH | NBD_FLAG_SEND_FUA).await; // enables flush and FUA
    println!("wrote size");

    write_u64(&mut stream, NBD_REPLY_MAGIC).await;
    write_u32(&mut stream, NBD_OPT_GO).await;
    write_u32(&mut stream, NBD_REP_ACK).await;
    write_u32(&mut stream, 0).await; // make it easy, set to 0
    println!("handshake completed");

    // =========================== transmission ===========================
    
    // fork: COPY ON WRITE
    // if you want an export called "foo_fork" when "foo" already exists,
    // you created a fork. This fork doesn't allocate real space until
    // the first write request.
    let do_fork = export_name.find(EXPORT_SUFFIX).is_some();
    let mut export_name_suffix_removed: String = export_name.clone();
    if do_fork {
        let idx = export_name.find(EXPORT_SUFFIX).expect("failed to get suffix index");
        export_name_suffix_removed = export_name[..idx].to_string();
    }
    let bucket = s3::S3Bucket::new();

    let mut file_handler = if do_fork &&
                              bucket.object_exists(&(export_name_suffix_removed.clone() + ".img")) &&
                              !bucket.object_exists(&(export_name.clone() + ".img")) {
        println!("creating fork of {}", export_name_suffix_removed);
        FileHandler::fork(&export_name_suffix_removed)
    } else {
        FileHandler::new(&export_name)
    };

    loop {
        // read header
        let mut header = ReqHeader {..Default::default()};
        read_header(&mut stream, &mut header).await;
        println!("got request");
        println!("header {:?}", header);

        while token_bucket.load(Ordering::SeqCst) == 0 {
            println!("out of tokens, sleep for {}ms", SLEEP_TIME);
            sleep(Duration::from_millis(SLEEP_TIME)).await;
        }
        token_bucket.fetch_sub(1, Ordering::SeqCst);

        match header.type_ {
            NBD_CMD_READ => {
                let mut buf = vec![0u8; header.length as usize];
                let bytes = file_handler.read(header.offset, &mut buf);
                println!("buffer size {}", buf[..].len());
                println!("read {} bytes from file", bytes);
                if bytes != header.length as usize {
                    println!("warning: bytes read ({}) != header.length ({})", bytes, header.length);
                }
                reply(&mut stream, header.cookie, Some(&mut buf[..])).await;
                println!("successfully read");
            },
            NBD_CMD_WRITE => {
                let mut buf = vec![0u8; header.length as usize];
                let mut to_read: isize = header.length as isize;
                let mut ptr = 0usize;
                while to_read > 0 {
                    let read = stream.read(&mut buf[ptr..]).await.expect("failed to read from stream") as isize;
                    to_read -= read;
                    ptr += read as usize;
                }
                let bytes = file_handler.write(header.offset, &mut buf);
                if header.comm_flags & NBD_CMD_FLAG_FUA != 0 {
                    // write through
                    println!("FUAing");
                    file_handler.flush();
                }
                println!("wrote {} bytes", bytes);
                reply(&mut stream, header.cookie, None).await;
                println!("successfully wrote");
            },
            NBD_CMD_DISC => {
                println!("successfully disconnected");
                return;
            },
            NBD_CMD_FLUSH => {
                file_handler.flush();
                reply(&mut stream, header.cookie, None).await;
                println!("successfully flushed");
            },
            _ => {
                reply(&mut stream, header.cookie, None).await;
                println!("successfully handled request type {}", header.type_);
            }
        }
    }
    
}

#[tokio::main]
async fn main() {
    println!("starting server");
    let listener = TcpListener::bind(URL).await.expect("failed to open listener");
    let token_buckets:Arc<Mutex<Vec<Arc<AtomicUsize>>>> = Arc::new(Mutex::new(vec![]));
    let token_buckets_add = token_buckets.clone();
    let mut idx = 0;

    // TODO: make it a separate thread
    // token bucket task
    tokio::spawn(async move {
        loop {
            {
                let tbs = token_buckets_add.lock().await;
                let mut tbs_idx = 0;
                if tbs.len() != 0 {
                    for token_bucket in &*tbs {
                        print!("token[{}] = {} ", tbs_idx, token_bucket.load(Ordering::SeqCst));
                        token_bucket.store(TOKEN_MX, Ordering::SeqCst);
                        tbs_idx += 1;
                    }
                    println!();
                }
            } // lock
            sleep(Duration::from_millis(TOKEN_REFILL_TIME)).await;
        }
    });

    // network IO task
    loop {
        let (stream, _) = listener.accept().await.expect("await listener failed");

        token_buckets.lock().await.push(Arc::new(AtomicUsize::new(TOKEN_MX)));
        let token_bucket = token_buckets.lock().await[idx].clone();

        tokio::spawn(async move {
            handle_traffic(stream, token_bucket).await;
        });
        idx += 1;
    }
}