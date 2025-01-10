use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::fs::PermissionsExt;
use std::sync::mpsc;
use std::thread;
use std::path::Path;
use std::fs;
use std::env;
use rustface::{Detector, ImageData};
use base64::{Engine as _, engine::general_purpose};
use serde::{Serialize, Deserialize};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

const MAX_WORKERS: usize = 5;
const SOCKET_PATH: &str = "/tmp/face_detect.sock";

// 线程池实现
struct ThreadPool {
    workers: Vec<Worker>,
    sender: mpsc::Sender<Message>,
}

struct Worker {
    #[allow(dead_code)]
    id: usize,
    thread: Option<thread::JoinHandle<()>>,
}

enum Message {
    NewJob(UnixStream),
    Terminate,
}

impl ThreadPool {
    fn new(size: usize) -> ThreadPool {
        assert!(size > 0);

        let (sender, receiver) = mpsc::channel();
        let receiver = std::sync::Arc::new(std::sync::Mutex::new(receiver));

        let mut workers = Vec::with_capacity(size);

        for id in 0..size {
            workers.push(Worker::new(id, std::sync::Arc::clone(&receiver)));
        }

        ThreadPool { workers, sender }
    }

    fn execute(&self, stream: UnixStream) {
        self.sender.send(Message::NewJob(stream)).unwrap();
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        // 发送终止消息给所有worker
        for _ in &self.workers {
            self.sender.send(Message::Terminate).unwrap();
        }

        // 等待所有线程完成
        for worker in &mut self.workers {
            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
            }
        }

        // 清理socket文件
        if Path::new(SOCKET_PATH).exists() {
            fs::remove_file(SOCKET_PATH).unwrap_or_else(|e| {
                eprintln!("Failed to remove socket file: {}", e);
            });
        }
    }
}

impl Worker {
    fn new(id: usize, receiver: std::sync::Arc<std::sync::Mutex<mpsc::Receiver<Message>>>) -> Worker {
        let thread = thread::spawn(move || {
            // 每个worker创建自己的detector实例
            let model_path = std::env::current_dir()
                .unwrap()
                .join("model")
                .join("seeta_fd_frontal_v1.0.bin");
            let mut detector = match rustface::create_detector(model_path.to_str().unwrap()) {
                Ok(detector) => {
                    let mut d = detector;
                    d.set_min_face_size(20);
                    d.set_score_thresh(2.0);
                    d.set_pyramid_scale_factor(0.8);
                    d.set_slide_window_step(4, 4);
                    d
                }
                Err(e) => {
                    eprintln!("Worker {} failed to create detector: {}", id, e);
                    return;
                }
            };

            loop {
                let message = receiver.lock().unwrap().recv().unwrap();

                match message {
                    Message::NewJob(stream) => {
                        println!("Worker {} got a job", id);
                        handle_client(stream, &mut detector);
                    }
                    Message::Terminate => {
                        println!("Worker {} was told to terminate", id);
                        break;
                    }
                }
            }
        });

        Worker {
            id,
            thread: Some(thread),
        }
    }
}

// 请求消息格式
#[derive(Deserialize)]
struct Request {
    image: String,  // base64 encoded image
}

// 响应消息格式
#[derive(Serialize)]
struct Response {
    #[allow(dead_code)]
    face_count: usize,
    #[allow(dead_code)]
    detect_time_ms: u64,
}

fn handle_client(mut stream: UnixStream, detector: &mut Box<dyn Detector>) {
    let mut header_buf = [0u8; 4];  // 4字节的消息长度头

    loop {
        // 读取消息长度
        match stream.read_exact(&mut header_buf) {
            Ok(_) => {
                let msg_len = (&header_buf[..]).read_u32::<BigEndian>().unwrap() as usize;
                
                // 读取消息体
                let mut msg_buf = vec![0u8; msg_len];
                if let Err(e) = stream.read_exact(&mut msg_buf) {
                    eprintln!("Failed to read message: {}", e);
                    break;
                }

                // 解析请求
                let request: Request = match serde_json::from_slice(&msg_buf) {
                    Ok(req) => req,
                    Err(e) => {
                        eprintln!("Failed to parse request: {}", e);
                        continue;
                    }
                };

                // 解码图片
                let image_data = match general_purpose::STANDARD.decode(&request.image) {
                    Ok(data) => data,
                    Err(e) => {
                        eprintln!("Failed to decode image: {}", e);
                        continue;
                    }
                };

                // 加载图片
                let img = match image::load_from_memory(&image_data) {
                    Ok(img) => img,
                    Err(e) => {
                        eprintln!("Failed to load image: {}", e);
                        continue;
                    }
                };

                // 转换为灰度图
                let gray = img.to_luma8();
                let (width, height) = gray.dimensions();
                let bytes = gray.into_raw();

                // 执行检测
                let detect_start = std::time::Instant::now();
                let image = ImageData::new(&bytes, width, height);
                let faces = detector.detect(&image);
                let detect_time = detect_start.elapsed();

                // 构造响应
                let response = Response {
                    face_count: faces.len(),
                    detect_time_ms: detect_time.as_millis() as u64,
                };

                // 序列化响应
                let response_data = match serde_json::to_vec(&response) {
                    Ok(data) => data,
                    Err(e) => {
                        eprintln!("Failed to serialize response: {}", e);
                        continue;
                    }
                };

                // 发送响应长度
                if let Err(e) = stream.write_u32::<BigEndian>(response_data.len() as u32) {
                    eprintln!("Failed to write response length: {}", e);
                    break;
                }

                // 发送响应数据
                if let Err(e) = stream.write_all(&response_data) {
                    eprintln!("Failed to write response: {}", e);
                    break;
                }
            }
            Err(e) => {
                if e.kind() != std::io::ErrorKind::UnexpectedEof {
                    eprintln!("Failed to read header: {}", e);
                }
                break;
            }
        }
    }
}

fn main() {

    let max_workers = env::var("FACE_DETECT_MAX_WORK")
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or(MAX_WORKERS);
    // 如果socket文件已存在，先删除
    if Path::new(SOCKET_PATH).exists() {
        fs::remove_file(SOCKET_PATH).unwrap_or_else(|e| {
            eprintln!("Failed to remove existing socket file: {}", e);
            std::process::exit(1);
        });
    }

    // 创建线程池
    let pool = ThreadPool::new(max_workers);

    // 创建 Unix Domain Socket 监听器
    let listener = UnixListener::bind(SOCKET_PATH).unwrap_or_else(|e| {
        eprintln!("Failed to create socket: {}", e);
        std::process::exit(1);
    });
    println!("Server listening on {} with {} workers", SOCKET_PATH, MAX_WORKERS);

    // 设置socket文件权限为666（所有用户可读写）
    fs::set_permissions(SOCKET_PATH, fs::Permissions::from_mode(0o666)).unwrap_or_else(|e| {
        eprintln!("Failed to set socket permissions: {}", e);
        std::process::exit(1);
    });

    // 接受连接
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                pool.execute(stream);
            }
            Err(e) => {
                eprintln!("Failed to accept connection: {}", e);
            }
        }
    }
}