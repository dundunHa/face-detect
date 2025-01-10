# face-detect

一个基于 rustface 的人脸检测服务。

## 功能特点

- 使用 Unix Domain Socket 进行通信
- 多线程处理请求,默认最大支持5个并发工作线程,通过设置环境变量 FACE_DETECT_MAX_WORK 修改最大的线程数
- 支持 base64 编码的图片输入
- 返回检测到的人脸数量和检测耗时

## 使用方法

1. 启动服务:
```bash
./face-detect
```

2. 发送请求:
```bash
echo -n "{\"image\":\"data:image/jpeg;base64,...\"}" | nc -U /tmp/face_detect.sock
```

## 返回格式

服务以 JSON 格式返回结果:

```json
{
  "face_count": 2,
  "detect_time_ms": 156
}
```

## 构建说明

1. 安装 Rust 工具链:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

2. 编译项目:
```bash
cargo build --release
```

编译后的二进制文件位于 `target/release/face-detect`

## 支持平台

- Linux (x86_64, aarch64, armv7)
- macOS (x86_64, arm64)
- Windows (x86_64)

## License

MIT