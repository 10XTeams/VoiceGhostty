//! 一键下载内置 whisper 的模型(whisper.cpp 的 ggml 格式)。
//!
//! - **两个下载源依次尝试**:huggingface.co,连不上再用国内镜像 hf-mirror.com(两边文件逐字节相同)。
//! - **断点续传**:先写 `<文件名>.part`,中断后再下从已有长度接着要(HTTP Range);换下载源也接着下。
//! - **自动重连**:传输中途断开时,只要这次连接下到了东西,就马上在同一个源上接着下(不限次数);
//!   连续几次一点进展都没有才换下一个源。几百 MB 的文件在不稳定的网络上断几次很正常,不该每次都让用户点重试。
//!   `.part` 不以 `.bin` 结尾,所以下到一半的文件永远不会被当成模型载入。
//! - **校验**:长度与服务器给的一致,且文件头是 ggml 魔数 `lmgg`(防止把出错页面当模型存下来),
//!   通过后才改名成正式文件名。
//! - **代理**:环境变量(`HTTPS_PROXY` 等)优先,没有时在 macOS 上读系统代理(`scutil --proxy`)——
//!   从 Finder 启动的 App 没有 shell 的环境变量,而 huggingface 在国内多半要走代理。
//!   本机地址永远直连。
//!
//! 下载在后台线程里跑,界面每帧读 [`Download::phase`] 显示进度。

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 一个可下载的模型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    /// 文件名,也是下载地址的最后一段
    pub file: &'static str,
    pub title: &'static str,
    /// 字节数(2026-11 在两个下载源上实测),下载前显示用;进度以服务器给的长度为准
    pub bytes: u64,
    pub note: &'static str,
}

/// 可选模型,第一个是推荐项
pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        file: "ggml-large-v3-turbo-q5_0.bin",
        title: "large-v3-turbo 量化版",
        bytes: 574_041_195,
        note: "中英文都准,体积适中",
    },
    ModelSpec {
        file: "ggml-large-v3-turbo.bin",
        title: "large-v3-turbo",
        bytes: 1_624_555_275,
        note: "和上面同一个模型的完整精度,略准,体积约三倍",
    },
    ModelSpec { file: "ggml-small.bin", title: "small", bytes: 487_601_967, note: "中文够用,识别更快" },
    ModelSpec { file: "ggml-base.bin", title: "base", bytes: 147_951_465, note: "最小最快,适合只说英文" },
];

/// 下载源,按顺序尝试
pub const SOURCES: &[&str] = &[
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/",
    "https://hf-mirror.com/ggerganov/whisper.cpp/resolve/main/",
];

/// 同一个源连续这么多次没下到任何东西,就换下一个源
const MAX_STALLED_ATTEMPTS: u32 = 3;
/// 两次重连之间等多久(没有进展时才等;有进展立即重连)
const RETRY_DELAY: Duration = if cfg!(test) { Duration::from_millis(50) } else { Duration::from_secs(3) };

/// ggml 模型文件头:uint32 0x67676d6c 按小端写出
const GGML_MAGIC: &[u8; 4] = b"lmgg";

/// 下载进行到哪一步
#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    /// 正在连接某个下载源(主机名)
    Connecting(String),
    /// 传输中断,马上重连同一个源接着下
    Retrying { host: String, reason: String },
    Downloading {
        host: String,
        done: u64,
        /// 服务器没给长度时为 None
        total: Option<u64>,
        /// 这一次连接以来的平均速度,字节/秒
        speed: f64,
    },
    Verifying,
    Done(PathBuf),
    Failed(String),
    Cancelled,
}

impl Phase {
    pub fn is_running(&self) -> bool {
        matches!(self, Phase::Connecting(_) | Phase::Retrying { .. } | Phase::Downloading { .. } | Phase::Verifying)
    }
}

/// 一次后台下载。drop 不会停止它;要停用 [`Download::cancel`]
pub struct Download {
    pub spec: ModelSpec,
    phase: Arc<Mutex<Phase>>,
    cancel: Arc<AtomicBool>,
}

impl Download {
    /// 开始把 `spec` 下载到 `dir`。`notify` 在进度变化时被调用(界面用它请求重绘),最多每 100 ms 一次
    pub fn start(spec: ModelSpec, dir: PathBuf, notify: impl Fn() + Send + 'static) -> Download {
        Download::start_from(spec, dir, SOURCES.iter().map(|s| s.to_string()).collect(), notify)
    }

    /// 同 [`Download::start`],但指定下载源(测试用本机服务器)
    pub fn start_from(spec: ModelSpec, dir: PathBuf, sources: Vec<String>, notify: impl Fn() + Send + 'static) -> Download {
        let phase = Arc::new(Mutex::new(Phase::Connecting(String::new())));
        let cancel = Arc::new(AtomicBool::new(false));
        let job = Job { file: spec.file, dir, sources, phase: phase.clone(), cancel: cancel.clone(), notify: Box::new(notify) };
        std::thread::Builder::new()
            .name("model-download".into())
            .spawn(move || job.run())
            .expect("spawn model download thread");
        Download { spec, phase, cancel }
    }

    pub fn phase(&self) -> Phase {
        self.phase.lock().unwrap().clone()
    }

    /// 停止下载。已下载的部分留在 `.part` 里,下次接着下
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

struct Job {
    file: &'static str,
    dir: PathBuf,
    sources: Vec<String>,
    phase: Arc<Mutex<Phase>>,
    cancel: Arc<AtomicBool>,
    notify: Box<dyn Fn() + Send>,
}

/// 一个下载源失败的原因
enum Failure {
    Cancelled,
    /// 这个源不行,换下一个
    Source(String),
    /// 本地出错(磁盘满、没权限),换源也没用
    Local(String),
}

impl Job {
    fn set(&self, phase: Phase) {
        *self.phase.lock().unwrap() = phase;
        (self.notify)();
    }

    fn run(self) {
        let result = self.download();
        let phase = match result {
            Ok(path) => Phase::Done(path),
            Err(Failure::Cancelled) => Phase::Cancelled,
            Err(Failure::Source(e) | Failure::Local(e)) => Phase::Failed(e),
        };
        self.set(phase);
    }

    fn download(&self) -> Result<PathBuf, Failure> {
        let target = self.dir.join(self.file);
        if is_ggml_model(&target) {
            return Ok(target); // 已经下过了
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| Failure::Local(format!("建不了模型目录 {}:{e}", self.dir.display())))?;
        let part = self.dir.join(format!("{}.part", self.file));
        let client = client().map_err(Failure::Local)?;

        let mut errors = vec![];
        let mut fetched = false;
        'sources: for base in &self.sources {
            let url = format!("{base}{}", self.file);
            let host = host_of(&url);
            let mut stalled = 0;
            loop {
                let before = part_len(&part);
                match self.fetch(&client, &url, &part) {
                    Ok(()) => {
                        fetched = true;
                        break 'sources;
                    }
                    Err(Failure::Source(e)) => {
                        log::warn!("model download: {url}: {e}");
                        if part_len(&part) > before {
                            stalled = 0; // 有进展:立即接着下
                        } else {
                            stalled += 1;
                            if stalled >= MAX_STALLED_ATTEMPTS {
                                errors.push(format!("{host}:{e}"));
                                continue 'sources;
                            }
                            self.pause(RETRY_DELAY)?;
                        }
                        self.set(Phase::Retrying { host: host.clone(), reason: e });
                    }
                    Err(other) => return Err(other),
                }
            }
        }
        if !fetched {
            return Err(Failure::Source(format!(
                "下载失败({})。检查网络或代理后重试,已下载的部分会保留,重试时接着下",
                errors.join(";")
            )));
        }

        self.set(Phase::Verifying);
        if !is_ggml_model(&part) {
            let _ = std::fs::remove_file(&part);
            return Err(Failure::Source("下载到的不是 whisper 模型文件(可能是代理或下载源返回了错误页面),已删除,请重试".into()));
        }
        std::fs::rename(&part, &target).map_err(|e| Failure::Local(format!("保存模型失败:{e}")))?;
        Ok(target)
    }

    /// 等一会儿,期间随时响应取消
    fn pause(&self, d: Duration) -> Result<(), Failure> {
        let until = Instant::now() + d;
        while Instant::now() < until {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(Failure::Cancelled);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    }

    /// 从一个下载源把文件接着下到 `part`
    fn fetch(&self, client: &reqwest::blocking::Client, url: &str, part: &Path) -> Result<(), Failure> {
        let host = host_of(url);
        if !matches!(*self.phase.lock().unwrap(), Phase::Retrying { .. }) {
            self.set(Phase::Connecting(host.clone()));
        }
        let have = part_len(part);
        let mut req = client.get(url);
        if have > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }
        let mut resp = req.send().map_err(|e| Failure::Source(describe(&e)))?;
        let status = resp.status();
        let (mut file, start) = match status.as_u16() {
            // 接着下
            206 => (OpenOptions::new().append(true).open(part).map_err(local)?, have),
            // 服务器不支持续传,或者本来就是从头下
            200 => (File::create(part).map_err(local)?, 0),
            // 请求的起点已经在文件末尾:上次其实下完了,只是没来得及校验改名
            416 if have > 0 => return Ok(()),
            _ => return Err(Failure::Source(format!("HTTP {status}"))),
        };
        let total = resp.content_length().map(|n| n + start);

        let began = Instant::now();
        let mut last_report = Instant::now() - Duration::from_secs(1);
        let mut done = start;
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(Failure::Cancelled);
            }
            let n = match resp.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => return Err(Failure::Source(format!("传输中断({})", innermost(&e)))),
            };
            file.write_all(&buf[..n]).map_err(local)?;
            done += n as u64;
            if last_report.elapsed() >= Duration::from_millis(100) {
                last_report = Instant::now();
                let speed = (done - start) as f64 / began.elapsed().as_secs_f64().max(0.001);
                self.set(Phase::Downloading { host: host.clone(), done, total, speed });
            }
        }
        file.flush().map_err(local)?;
        if let Some(total) = total {
            if done != total {
                return Err(Failure::Source(format!("连接提前断开({done} / {total} 字节)")));
            }
        }
        Ok(())
    }
}

fn part_len(part: &Path) -> u64 {
    part.metadata().map(|m| m.len()).unwrap_or(0)
}

fn local(e: std::io::Error) -> Failure {
    Failure::Local(format!("写入模型文件失败:{e}"))
}

/// 文件存在且以 ggml 魔数开头
pub fn is_ggml_model(path: &Path) -> bool {
    let mut head = [0u8; 4];
    File::open(path).and_then(|mut f| f.read_exact(&mut head)).is_ok() && &head == GGML_MAGIC
}

fn host_of(url: &str) -> String {
    reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| url.to_string())
}

/// 把 reqwest 的错误说成人话
fn describe(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "超时".into()
    } else if e.is_connect() {
        "连不上".into()
    } else {
        innermost(e)
    }
}

/// 错误链最里层的那句(外层多是「request or response body error」这种没信息量的包装)
fn innermost(e: &(dyn std::error::Error + 'static)) -> String {
    let mut msg = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        msg = s.to_string();
        source = s.source();
    }
    msg
}

fn client() -> Result<reqwest::blocking::Client, String> {
    let mut builder = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        // 阻塞版 reqwest 的 timeout 作用于每一次读,不是整个下载:30 秒没收到一个字节才算卡住
        .timeout(Duration::from_secs(30))
        .no_proxy();
    if let Some(proxy) = proxy_url() {
        log::info!("model download: using proxy {proxy}");
        let p = reqwest::Proxy::custom(move |url| {
            let local = url.host_str().is_some_and(|h| h == "localhost" || h.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback()));
            (!local).then(|| proxy.clone())
        });
        builder = builder.proxy(p);
    }
    builder.build().map_err(|e| format!("初始化网络失败:{e}"))
}

/// 要用的代理:环境变量优先,其次 macOS 系统代理
fn proxy_url() -> Option<String> {
    for key in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy"] {
        if let Ok(v) = std::env::var(key) {
            if !v.trim().is_empty() {
                return Some(v.trim().to_string());
            }
        }
    }
    if cfg!(target_os = "macos") {
        let out = std::process::Command::new("scutil").arg("--proxy").output().ok()?;
        return parse_scutil_proxy(&String::from_utf8_lossy(&out.stdout));
    }
    None
}

/// 从 `scutil --proxy` 的输出里取 HTTPS 代理(没开就取 HTTP 代理)
fn parse_scutil_proxy(text: &str) -> Option<String> {
    let field = |key: &str| {
        text.lines().find_map(|l| {
            let (k, v) = l.split_once(" : ")?;
            (k.trim() == key).then(|| v.trim().to_string())
        })
    };
    for scheme in ["HTTPS", "HTTP"] {
        if field(&format!("{scheme}Enable")).as_deref() == Some("1") {
            let host = field(&format!("{scheme}Proxy"))?;
            let port = field(&format!("{scheme}Port")).unwrap_or_else(|| "80".into());
            return Some(format!("http://{host}:{port}"));
        }
    }
    None
}

/// 「35 KB」「123 MB」「1.6 GB」(下载速度也用它,慢网下不能显示成 0 MB)
pub fn human_bytes(n: u64) -> String {
    let mb = n as f64 / 1_000_000.0;
    if mb >= 1000.0 {
        format!("{:.1} GB", mb / 1000.0)
    } else if mb >= 1.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{:.0} KB", n as f64 / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;

    #[test]
    fn parses_system_proxy() {
        let out = "<dictionary> {\n  HTTPEnable : 1\n  HTTPPort : 7897\n  HTTPProxy : 127.0.0.1\n  HTTPSEnable : 1\n  HTTPSPort : 7898\n  HTTPSProxy : 10.0.0.2\n}";
        assert_eq!(parse_scutil_proxy(out).as_deref(), Some("http://10.0.0.2:7898"));
        let http_only = "  HTTPEnable : 1\n  HTTPPort : 8080\n  HTTPProxy : proxy.lan\n  HTTPSEnable : 0\n";
        assert_eq!(parse_scutil_proxy(http_only).as_deref(), Some("http://proxy.lan:8080"));
        assert_eq!(parse_scutil_proxy("  HTTPEnable : 0\n  HTTPSEnable : 0\n"), None);
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human_bytes(574_041_195), "574 MB");
        assert_eq!(human_bytes(1_624_555_275), "1.6 GB");
        assert_eq!(human_bytes(31_258), "31 KB");
    }

    /// 本机 HTTP 服务器:提供 `body`,支持 Range;`cut` 为 Some(n) 时第一次请求只发 n 字节就断开。
    /// 返回地址和「每次请求的起始字节」记录
    fn serve(body: Vec<u8>, cut: Option<usize>) -> (String, Arc<Mutex<Vec<usize>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let starts = Arc::new(Mutex::new(vec![]));
        let log = starts.clone();
        std::thread::spawn(move || {
            let mut first = true;
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut from = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        from = v.trim().trim_end_matches('-').parse().unwrap();
                    }
                }
                log.lock().unwrap().push(from);
                let slice = &body[from.min(body.len())..];
                let head = if from > 0 {
                    format!("HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", slice.len())
                } else {
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", slice.len())
                };
                let _ = stream.write_all(head.as_bytes());
                let send = match (first, cut) {
                    (true, Some(n)) => &slice[..n],
                    _ => slice,
                };
                first = false;
                let _ = stream.write_all(send);
            }
        });
        (format!("http://{addr}/"), starts)
    }

    fn fake_model(len: usize) -> Vec<u8> {
        let mut v = GGML_MAGIC.to_vec();
        v.extend((0..len - 4).map(|i| (i % 251) as u8));
        v
    }

    fn wait(d: &Download) -> Phase {
        let deadline = Instant::now() + Duration::from_secs(20);
        while d.phase().is_running() || d.phase() == Phase::Connecting(String::new()) {
            assert!(Instant::now() < deadline, "download did not finish: {:?}", d.phase());
            std::thread::sleep(Duration::from_millis(20));
        }
        d.phase()
    }

    const SPEC: ModelSpec = ModelSpec { file: "ggml-test.bin", title: "test", bytes: 0, note: "" };

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vg-model-dl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn falls_back_to_second_source_and_resumes_after_a_drop() {
        let body = fake_model(300_000);
        // 第一个源:端口上没人听,每次都立即「连不上」→ 重试几次后换源;
        // 第二个源第一次只发 100 KB 就断 → 有进展,马上在同一个源上从断点接着下
        let dead = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            let a = l.local_addr().unwrap();
            drop(l);
            format!("http://{a}/")
        };
        let (good, starts) = serve(body.clone(), Some(100_000));
        let dir = temp_dir("resume");

        let d = Download::start_from(SPEC, dir.clone(), vec![dead, good], || {});
        let path = dir.join("ggml-test.bin");
        assert_eq!(wait(&d), Phase::Done(path.clone()));
        assert_eq!(std::fs::read(&path).unwrap(), body);
        assert!(!dir.join("ggml-test.bin.part").exists());
        assert_eq!(*starts.lock().unwrap(), vec![0, 100_000], "断开后应从断点续传,而不是从头下");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resumes_a_part_left_by_an_earlier_run() {
        let body = fake_model(50_000);
        let (src, starts) = serve(body.clone(), None);
        let dir = temp_dir("leftover");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ggml-test.bin.part"), &body[..20_000]).unwrap();
        let d = Download::start_from(SPEC, dir.clone(), vec![src], || {});
        assert_eq!(wait(&d), Phase::Done(dir.join("ggml-test.bin")));
        assert_eq!(std::fs::read(dir.join("ggml-test.bin")).unwrap(), body);
        assert_eq!(*starts.lock().unwrap(), vec![20_000]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gives_up_after_every_source_stalls() {
        let dead = |_| {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            let a = l.local_addr().unwrap();
            drop(l);
            format!("http://{a}/")
        };
        let dir = temp_dir("dead");
        let d = Download::start_from(SPEC, dir.clone(), (0..2).map(dead).collect(), || {});
        match wait(&d) {
            Phase::Failed(msg) => assert!(msg.contains("连不上") && msg.contains("下载失败"), "{msg}"),
            other => panic!("{other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_a_file_that_is_not_a_model() {
        let (src, _) = serve(b"<html>rate limited</html>".to_vec(), None);
        let dir = temp_dir("reject");
        let d = Download::start_from(SPEC, dir.clone(), vec![src], || {});
        match wait(&d) {
            Phase::Failed(msg) => assert!(msg.contains("不是 whisper 模型"), "{msg}"),
            other => panic!("{other:?}"),
        }
        assert!(!dir.join("ggml-test.bin").exists() && !dir.join("ggml-test.bin.part").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn already_downloaded_is_done_immediately() {
        let dir = temp_dir("existing");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ggml-test.bin"), fake_model(100)).unwrap();
        let d = Download::start_from(SPEC, dir.clone(), vec!["http://127.0.0.1:9/".into()], || {});
        assert_eq!(wait(&d), Phase::Done(dir.join("ggml-test.bin")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
