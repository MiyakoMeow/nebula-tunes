//! Nebula Tunes library target used for WASM compilation checks.

pub mod config;

#[cfg(target_arch = "wasm32")]
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
