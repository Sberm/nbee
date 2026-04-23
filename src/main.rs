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
use std::io::{Write, Read};

const NBDMAGIC: u64 = 0x4e42444d41474943;
const IHAVEOPT: u64 = 0x49484156454F5054;
const NBD_FLAG_FIXED_NEWSTYLE: u16 = 1 << 0;
const NBD_REPLY_MAGIC: u64 = 0x3e889045565a9;
const NBD_REP_ACK: u32 = 0;

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

fn print_client_input(stream: &mut TcpStream) {
    // debug
    let mut debug_buf: [u8; 1024] = [0; 1024];
    let _ = stream.read_exact(&mut debug_buf);
    for i in 0..1024/64 {
        for j in 0..64 {
            print!("{}", debug_buf[i * 64 + j]);
        }
        println!();
    }
}

fn main() {
    println!("starting server");
    let listener = TcpListener::bind("127.0.0.1:10809").expect("failed to open listener");
    for _stream in listener.incoming() {
        // =========================== handshake ===========================
        println!("got packet");
        let mut stream = _stream.expect("no stream");

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
        let client_opts = read_u32(&mut stream);
        println!("client_opts {}", client_opts);
        let client_opts_len = read_u32(&mut stream);
        println!("client_opts_len {}", client_opts_len);
        let mut opts_buf: [u8; 2048] = [0; 2048];
        let _ = stream.read_exact(&mut opts_buf[..(client_opts_len as usize)]);
        println!("client opts buf:");
        for i in 0..(client_opts_len as usize) {
            if opts_buf[i] <= 32 {
                println!("{}", opts_buf[i]);
            } else {
                print!("{}", opts_buf[i] as char)
            }
        }
        println!();

        // server responds
        write_u64(&mut stream, NBD_REPLY_MAGIC);
        write_u32(&mut stream, client_opts);
        write_u32(&mut stream, NBD_REP_ACK);
        write_u32(&mut stream, 0); // make it easy, set to 0

        // =========================== transmission ===========================

    }
    // transmission
}