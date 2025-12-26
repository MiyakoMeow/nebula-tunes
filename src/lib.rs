//! Nebula Tunes library target used for WASM compilation checks.

#[cfg(not(target_os = "wasi"))]
pub mod config;

#[cfg(target_os = "wasi")]
/// WASM 构建冒烟检查入口
///
/// # Errors
///
/// - `getrandom` 获取随机数失败
pub fn wasm_smoke_checks() -> Result<(), getrandom::Error> {
    let _ = bms_rs::bms::default_config();
    let mut buf = [0u8; 16];
    getrandom::fill(&mut buf)?;
    Ok(())
}
