//! 読みの符号化。読みはほぼすべてひらがななので、ひらがなを 1 文字 1 バイトで持つ
//! （UTF-8 では 3 バイト）。
//!
//! ```text
//! 0x01..=0x7F  ASCII
//! 0x80..=0xD5  ひらがな U+3041..=U+3096
//! 0xD6         長音 ー
//! 0xFE x       カタカナ（x はひらがなのバイト。U+30A1..=U+30F6）
//! 0xFF n …     そのほかの文字の UTF-8 を n バイト
//! ```

const HIRAGANA_FIRST: u32 = 0x3041;
const HIRAGANA_LAST: u32 = 0x3096;
const BASE: u8 = 0x80;
const LONG: u8 = 0xD6;
const KATAKANA: u8 = 0xFE;
const ESCAPE: u8 = 0xFF;

fn hiragana_byte(cp: u32) -> Option<u8> {
    (HIRAGANA_FIRST..=HIRAGANA_LAST)
        .contains(&cp)
        .then(|| BASE + (cp - HIRAGANA_FIRST) as u8)
}

/// 読みを符号化する。
pub(crate) fn encode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() / 2);
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        let cp = c as u32;
        if (1..0x80).contains(&cp) {
            out.push(cp as u8);
        } else if let Some(b) = hiragana_byte(cp) {
            out.push(b);
        } else if c == 'ー' {
            out.push(LONG);
        } else if let Some(b) = cp.checked_sub(0x60).and_then(hiragana_byte) {
            out.push(KATAKANA);
            out.push(b);
        } else {
            // 続くかな以外の文字をまとめて 1 つの UTF-8 の塊にする
            let mut run = String::new();
            run.push(c);
            while let Some(&next) = chars.peek() {
                let ncp = next as u32;
                let is_kana = (1..0x80).contains(&ncp)
                    || hiragana_byte(ncp).is_some()
                    || next == 'ー'
                    || ncp.checked_sub(0x60).and_then(hiragana_byte).is_some();
                if is_kana || run.len() + next.len_utf8() > 255 {
                    break;
                }
                run.push(next);
                chars.next();
            }
            out.push(ESCAPE);
            out.push(run.len() as u8);
            out.extend_from_slice(run.as_bytes());
        }
    }
    out
}

/// 符号化した読みを元に戻す。形が正しくなければ `None`。
pub(crate) fn decode(bytes: &[u8]) -> Option<String> {
    let mut out = String::with_capacity(bytes.len() * 3);
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        match b {
            0x01..=0x7F => out.push(b as char),
            BASE..=0xD5 => out.push(char::from_u32(HIRAGANA_FIRST + (b - BASE) as u32)?),
            LONG => out.push('ー'),
            KATAKANA => {
                let h = *bytes.get(i)?;
                i += 1;
                if !(BASE..=0xD5).contains(&h) {
                    return None;
                }
                out.push(char::from_u32(HIRAGANA_FIRST + (h - BASE) as u32 + 0x60)?);
            }
            ESCAPE => {
                let n = *bytes.get(i)? as usize;
                let run = bytes.get(i + 1..i + 1 + n)?;
                out.push_str(std::str::from_utf8(run).ok()?);
                i += 1 + n;
            }
            _ => return None,
        }
    }
    Some(out)
}

/// 符号化した読みの形が正しいか（文字列を作らずに確かめる）。
pub(crate) fn is_valid(bytes: &[u8]) -> bool {
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        match b {
            0x01..=0x7F | BASE..=0xD5 | LONG => {}
            KATAKANA => match bytes.get(i) {
                Some(h) if (BASE..=0xD5).contains(h) => i += 1,
                _ => return false,
            },
            ESCAPE => {
                let Some(&n) = bytes.get(i) else { return false };
                let Some(run) = bytes.get(i + 1..i + 1 + n as usize) else {
                    return false;
                };
                if std::str::from_utf8(run).is_err() {
                    return false;
                }
                i += 1 + n as usize;
            }
            _ => return false,
        }
    }
    true
}
