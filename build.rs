/// 将 git 提交哈希和构建日期注入为编译时环境变量，
/// 供 `env!("GIT_HASH")` / `env!("BUILD_DATE")` 使用。
fn main() {
    // git 9 位短哈希
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short=9", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| String::from("unknown"));

    // 构建日期 YYYY-MM-DD
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();

    println!("cargo:rustc-env=GIT_HASH={}", hash);
    println!("cargo:rustc-env=BUILD_DATE={}", date);

    // HEAD 变化时自动重新运行 build script
    println!("cargo:rerun-if-changed=.git/HEAD");
}
