use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::fs;
use base64::{Engine as _, engine::general_purpose};
use serde::{Serialize, Deserialize};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

const SOCKET_PATH: &str = "/tmp/face_detect.sock";

#[derive(Serialize)]
struct Request {
    image: String,
}

#[derive(Deserialize, Debug)]
struct Response {
    face_count: usize,
    detect_time_ms: u64,
}

fn main() {
    // 连接服务器
    let mut stream = UnixStream::connect(SOCKET_PATH).expect("Failed to connect to server");

    // 读取图片文件
    let image_data = fs::read("/Users/lxp/Downloads/0f05bfcb8fb9b16ba7942c35e5221d18-low.jpg").expect("Failed to read image file");
    
    // Base64 编码
    let image_base64 = general_purpose::STANDARD.encode(&image_data);

    // 构造请求
    let request = Request {
        image: image_base64,
    };

    // 序列化请求
    let request_data = serde_json::to_vec(&request).expect("Failed to serialize request");

    // 发送请求长度
    stream.write_u32::<BigEndian>(request_data.len() as u32).expect("Failed to write length");

    // 发送请求数据
    stream.write_all(&request_data).expect("Failed to write request");

    // 读取响应长度
    let response_len = stream.read_u32::<BigEndian>().expect("Failed to read response length") as usize;

    // 读取响应数据
    let mut response_data = vec![0u8; response_len];
    stream.read_exact(&mut response_data).expect("Failed to read response");

    // 解析响应
    let response: Response = serde_json::from_slice(&response_data).expect("Failed to parse response");

    println!("Response: {:?}", response);
} 