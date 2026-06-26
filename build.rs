/// 将 git 提交哈希、构建日期、目标平台注入为编译时环境变量，
/// 供 `env!("GIT_HASH")` / `env!("BUILD_DATE")` / `env!("PLATFORM")` 使用。
fn main() {
    // git 9 位短哈希
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short=9", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| String::from("unknown"));

    // 构建日期 YYYY-MM-DD
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();

    // 目标平台（对齐 GitHub Release 的 artifact 命名）
    let platform = match std::env::var("TARGET").as_deref() {
        Ok("x86_64-unknown-linux-gnu") => "linux-x86_64",
        Ok("aarch64-unknown-linux-gnu") => "linux-arm64",
        Ok("x86_64-apple-darwin") => "darwin-x86_64",
        Ok("aarch64-apple-darwin") => "darwin-arm64",
        Ok("x86_64-pc-windows-msvc") => "windows-x64",
        _ => "unknown",
    };

    println!("cargo:rustc-env=GIT_HASH={}", hash);
    println!("cargo:rustc-env=BUILD_DATE={}", date);
    println!("cargo:rustc-env=PLATFORM={}", platform);

    // HEAD 变化时自动重新运行 build script
    println!("cargo:rerun-if-changed=.git/HEAD");
}