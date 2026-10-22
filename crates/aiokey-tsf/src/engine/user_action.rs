use crate::extension::VKeyExt;
use aiokey_keymap::Key;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardState, ToUnicode, VK_SHIFT};

/// 仮想キーコードをキーにする。キーの解釈そのものは OS に依存しない aiokey-keymap にある。
pub fn key_from_wparam(key_code: usize) -> Key {
    aiokey_keymap::key_from_vk(key_code as u32, VK_SHIFT.is_pressed(), || {
        let mut key_state = [0u8; 256];
        unsafe { GetKeyboardState(&mut key_state) }.ok()?;
        let mut unicode = [0u16; 1];
        unsafe { ToUnicode(key_code as u32, 0, Some(&key_state), &mut unicode, 0) };
        (unicode[0] != 0)
            .then(|| char::from_u32(unicode[0] as u32))
            .flatten()
    })
}
