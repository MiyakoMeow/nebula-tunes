//! 按键映射：将原始按键代码转换为轨道索引
//!
//! 负责维护配置的键位映射关系，并将原始输入事件转换为游戏逻辑输入。

use std::collections::HashMap;

use crate::loops::{InputMsg, KeyState, RawInputMsg, RawKeyCode};

/// 最大轨道数（8轨 BMS）
const MAX_LANES: usize = 8;

/// 按键映射器
pub struct KeyMap {
    /// 按键代码字符串到轨道索引的映射
    map: HashMap<String, usize>,
}

impl KeyMap {
    /// 从配置的按键代码列表创建映射器
    ///
    /// # 参数
    ///
    /// - `key_codes`: 按键代码字符串列表（如 `["KeyA", "KeyS", ...]`）
    ///
    /// # 返回
    ///
    /// 返回建立的按键映射器，最多支持 8 个轨道
    #[must_use]
    pub fn new(key_codes: Vec<String>) -> Self {
        let mut map = HashMap::new();
        for (idx, code) in key_codes.into_iter().enumerate().take(MAX_LANES) {
            map.insert(code, idx);
        }
        Self { map }
    }

    /// 将原始输入消息转换为语义化输入消息
    ///
    /// # 参数
    ///
    /// - `raw_msg`: 原始输入消息
    ///
    /// # 返回
    ///
    /// 如果按键代码在映射中存在，返回对应的 `InputMsg`；否则返回 `None`
    #[must_use]
    pub fn convert(&self, raw_msg: RawInputMsg) -> Option<InputMsg> {
        match raw_msg {
            RawInputMsg::Key { code, state } => {
                let RawKeyCode(key_str) = code;
                let idx = self.map.get(&key_str).copied()?;
                match state {
                    KeyState::Pressed => Some(InputMsg::KeyDown(idx)),
                    KeyState::Released => Some(InputMsg::KeyUp(idx)),
                }
            }
            // 鼠标、触控、手柄输入目前不转换为游戏逻辑输入
            RawInputMsg::Mouse { .. } | RawInputMsg::Touch { .. } | RawInputMsg::Gamepad { .. } => {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_map_conversion() {
        let key_map = KeyMap::new(vec!["KeyA".into(), "KeyS".into(), "Space".into()]);

        // 测试已映射的按键
        let msg = RawInputMsg::Key {
            code: RawKeyCode("KeyA".into()),
            state: KeyState::Pressed,
        };
        assert_eq!(key_map.convert(msg), Some(InputMsg::KeyDown(0)));

        // 测试未映射的按键
        let unmapped_msg = RawInputMsg::Key {
            code: RawKeyCode("KeyZ".into()),
            state: KeyState::Pressed,
        };
        assert_eq!(key_map.convert(unmapped_msg), None);
    }

    #[test]
    fn test_key_map_max_lanes() {
        // 测试超过8个按键的情况
        let keys: Vec<String> = (0..10).map(|i| format!("Key{}", i)).collect();
        let key_map = KeyMap::new(keys);

        // 第9个按键应该不在映射中
        let msg = RawInputMsg::Key {
            code: RawKeyCode("Key8".into()),
            state: KeyState::Pressed,
        };
        assert_eq!(key_map.convert(msg), None);
    }
}
