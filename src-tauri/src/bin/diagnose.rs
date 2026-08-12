//! `diagnose` —— 把 Codex / 上游 API 响应日志喂入诊断模块，输出结构化 JSON 报告。
//!
//! 用法：
//!   diagnose <upstream.log>                分析单段日志
//!   diagnose <upstream.log> <proxy.log>    上下游配对分析，定位代理层
//!   cat upstream.log | diagnose            从 stdin 读取单段日志
//!   diagnose --help                        打印日志获取说明
//!
//! 退出码：0 = 正常；2 = 用法错误。
//!
//! 注：本二进制作为 `src/bin/diagnose.rs` 由 cargo 自动发现，无需在 Cargo.toml
//! 显式声明 `[[bin]]`。

use std::io::Read;
use antigravity_tools_lib::diagnostics::{analyze_pair, analyze_transcript, how_to_capture_logs};

fn read_file_or_stdin(path: Option<&str>) -> Result<String, String> {
    match path {
        Some(p) => std::fs::read_to_string(p).map_err(|e| format!("无法读取 {}: {}", p, e)),
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| format!("读取 stdin 失败: {}", e))?;
            Ok(buf)
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{}", how_to_capture_logs());
        return;
    }

    let report = match args.len() {
        1 => {
            // stdin
            let input = match read_file_or_stdin(None) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("错误: {}", e);
                    std::process::exit(2);
                }
            };
            serde_json::to_string_pretty(&analyze_transcript(&input))
                .unwrap_or_else(|e| format!("序列化失败: {}", e))
        }
        2 => {
            let upstream = match read_file_or_stdin(Some(&args[1])) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("错误: {}", e);
                    std::process::exit(2);
                }
            };
            serde_json::to_string_pretty(&analyze_transcript(&upstream))
                .unwrap_or_else(|e| format!("序列化失败: {}", e))
        }
        3 => {
            let upstream = match read_file_or_stdin(Some(&args[1])) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("错误: {}", e);
                    std::process::exit(2);
                }
            };
            let proxy = match read_file_or_stdin(Some(&args[2])) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("错误: {}", e);
                    std::process::exit(2);
                }
            };
            serde_json::to_string_pretty(&analyze_pair(&upstream, &proxy))
                .unwrap_or_else(|e| format!("序列化失败: {}", e))
        }
        _ => {
            eprintln!(
                "用法:\n  diagnose <upstream.log>\n  diagnose <upstream.log> <proxy.log>\n  cat upstream.log | diagnose\n  diagnose --help"
            );
            std::process::exit(2);
        }
    };

    println!("{}", report);
}
