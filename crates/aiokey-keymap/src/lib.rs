//! キーの解釈と、変換の状態の移り変わり（OS に依存しない）。
//!
//! TSF（Windows）はキーを [`key_from_vk`] で [`Key`] にし、[`transition`] が返す操作を実行するだけにする。

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputMode {
    /// 直接入力
    #[default]
    Latin,
    /// AI-O Key の入力
    Kana,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompositionState {
    #[default]
    None,
    Composing,
    Previewing,
    Selecting,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClientAction {
    StartComposition,
    EndComposition,
    AppendText(String),
    RemoveText,
    ShrinkText(String),
    SetTextWithType(SetTextType),
    MoveCursor(i32),
    SetSelection(SetSelectionType),
    SetIMEMode(InputMode),
    Convert,
    SetContext(String),
    ShowCandidateWindow,
    HideCandidateWindow,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SetSelectionType {
    Up,
    Down,
    Number(i32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SetTextType {
    Hiragana,
    Katakana,
    HalfKatakana,
    FullLatin,
    HalfLatin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigation {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Function {
    Six,
    Seven,
    Eight,
    Nine,
    Ten,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Input(char),
    Separator,
    Backspace,
    Enter,
    Space,
    Tab,
    Escape,
    Navigation(Navigation),
    Function(Function),
    Number(i8),
    ToggleInputMode,
    DirectMode,
    AiokeyMode,
    Unknown,
}

/// 文脈として送る長さ（後ろからの文字数。段2の採点器が使う長さと同じ）。
pub const CONTEXT_CHARS: usize = 40;

/// Windows の仮想キーコードをキーにする。`to_char` は、ほかに当てはまらないキーの文字（ToUnicode）を返す。
pub fn key_from_vk(vk: u32, shift: bool, to_char: impl FnOnce() -> Option<char>) -> Key {
    match vk {
        0x08 => Key::Backspace,                                  // VK_BACK
        0x09 => Key::Tab,                                        // VK_TAB
        0x0D => Key::Enter,                                      // VK_RETURN
        0x1B => Key::Escape,                                     // VK_ESCAPE
        0x20 if shift => Key::Separator,                         // Shift+Space は区切り
        0x20 => Key::Space,                                      // VK_SPACE
        0x25 => Key::Navigation(Navigation::Left),               // VK_LEFT
        0x26 => Key::Navigation(Navigation::Up),                 // VK_UP
        0x27 => Key::Navigation(Navigation::Right),              // VK_RIGHT
        0x28 => Key::Navigation(Navigation::Down),               // VK_DOWN
        0x30..=0x39 if !shift => Key::Number((vk - 0x30) as i8), // VK_0..VK_9
        0x60..=0x69 => Key::Number((vk - 0x60) as i8),           // VK_NUMPAD0..9
        0x75 => Key::Function(Function::Six),                    // VK_F6
        0x76 => Key::Function(Function::Seven),                  // VK_F7
        0x77 => Key::Function(Function::Eight),                  // VK_F8
        0x78 => Key::Function(Function::Nine),                   // VK_F9
        0x79 => Key::Function(Function::Ten),                    // VK_F10
        // 半角／全角（VK_DBE_SBCSCHAR、VK_DBE_DBCSCHAR、Alt+半角／全角の VK_KANJI）
        0xF3 | 0xF4 | 0x19 => Key::ToggleInputMode,
        // 英数（VK_DBE_ALPHANUMERIC）、IME オフのキー（VK_IME_OFF）
        0xF0 | 0x1A => Key::DirectMode,
        // カタカナ／ひらがな（VK_DBE_KATAKANA、VK_DBE_HIRAGANA）、IME オンのキー（VK_IME_ON）
        0xF1 | 0xF2 | 0x16 => Key::AiokeyMode,
        _ => match to_char() {
            Some(c) if !c.is_control() => Key::Input(c),
            _ => Key::Unknown,
        },
    }
}

/// 変換の今の様子。TSF が持っている値を渡す。
pub struct Snapshot<'a> {
    pub state: CompositionState,
    pub mode: InputMode,
    /// 表示している候補
    pub preview: &'a str,
    /// 候補のうしろの、まだ変換していない部分
    pub suffix: &'a str,
    /// 確定した文（文脈）
    pub context: &'a str,
    /// 候補ウィンドウを開いているか
    pub window_open: bool,
    /// 選んでいる候補の位置
    pub selection: usize,
    /// 候補の一覧
    pub candidates: &'a [String],
}

/// 候補ウィンドウの 1 ページの候補の数（aiokey-ui の PAGE_SIZE と同じ）
pub const PAGE_SIZE: usize = 9;

/// 確定したあとの文脈（後ろの [`CONTEXT_CHARS`] 文字）。
fn committed(s: &Snapshot) -> ClientAction {
    committed_text(s, s.preview)
}

fn committed_text(s: &Snapshot, text: &str) -> ClientAction {
    let text: Vec<char> = s.context.chars().chain(text.chars()).collect();
    let start = text.len().saturating_sub(CONTEXT_CHARS);
    ClientAction::SetContext(text[start..].iter().collect())
}

/// キーを押したときの、次の状態と行う操作。IME で受けないキーなら None。
pub fn transition(s: &Snapshot, key: Key) -> Option<(CompositionState, Vec<ClientAction>)> {
    use ClientAction as A;
    use CompositionState as S;
    let kana = s.mode == InputMode::Kana;
    let toggled = match s.mode {
        InputMode::Kana => InputMode::Latin,
        InputMode::Latin => InputMode::Kana,
    };
    match s.state {
        S::None => match key {
            Key::Input(c) if kana => Some((
                S::Composing,
                vec![A::StartComposition, A::AppendText(c.to_string())],
            )),
            Key::Number(n) if kana => Some((
                S::Composing,
                vec![A::StartComposition, A::AppendText(n.to_string())],
            )),
            Key::ToggleInputMode => Some((S::None, vec![A::SetIMEMode(toggled)])),
            Key::DirectMode if kana => Some((S::None, vec![A::SetIMEMode(InputMode::Latin)])),
            Key::AiokeyMode if !kana => Some((S::None, vec![A::SetIMEMode(InputMode::Kana)])),
            _ => None,
        },
        S::Composing | S::Previewing => {
            let (next, mut actions) = composing(s, key)?;
            // 打っている間・確定・取り消しでは候補ウィンドウを閉じ、次の候補に進んだら開く
            let hide = A::HideCandidateWindow;
            if s.window_open
                && next != S::Previewing
                && !actions.is_empty()
                && !actions.contains(&hide)
            {
                actions.push(hide);
            }
            let moved = actions.iter().any(|a| matches!(a, A::SetSelection(_)));
            if !s.window_open && next == S::Previewing && moved {
                actions.push(A::ShowCandidateWindow);
            }
            Some((next, actions))
        }
        S::Selecting => None,
    }
}

/// 変換中のキー。候補ウィンドウの開け閉めは [`transition`] で足す。
fn composing(s: &Snapshot, key: Key) -> Option<(CompositionState, Vec<ClientAction>)> {
    use ClientAction as A;
    use CompositionState as S;
    let previewing = s.state == S::Previewing;
    // 候補ウィンドウを開いているときは、数字キーで表示中のページの候補を選んで確定する
    if let (Key::Number(n), true) = (key, s.window_open) {
        // 番号は 1〜9（0 は使わない）。表示中のページの先頭から数える
        let page_start = (s.selection / PAGE_SIZE) * PAGE_SIZE;
        let picked = (n as usize)
            .checked_sub(1)
            .map(|offset| page_start + offset)
            .and_then(|index| Some((index, s.candidates.get(index)?)));
        return Some(match picked {
            Some((index, text)) => (
                S::None,
                vec![
                    A::SetSelection(SetSelectionType::Number(index as i32)),
                    A::EndComposition,
                    committed_text(s, text),
                    A::HideCandidateWindow,
                ],
            ),
            _ => (s.state, vec![]),
        });
    }
    Some(match key {
        // 変換したあとに続けて打つと、表示している候補を確定して、新しく打ち始める
        Key::Input(c) if previewing => (
            S::Composing,
            vec![committed(s), A::ShrinkText(c.to_string())],
        ),
        Key::Number(n) if previewing => (
            S::Composing,
            vec![committed(s), A::ShrinkText(n.to_string())],
        ),
        Key::Input(c) => (S::Composing, vec![A::AppendText(c.to_string())]),
        Key::Number(n) => (S::Composing, vec![A::AppendText(n.to_string())]),
        Key::Separator => (
            S::Composing,
            vec![A::AppendText(aiokey_engine::input::SEPARATOR.to_string())],
        ),
        Key::Backspace if s.preview.chars().count() <= 1 => {
            (S::None, vec![A::RemoveText, A::EndComposition])
        }
        Key::Backspace => (S::Composing, vec![A::RemoveText]),
        Key::Enter if s.suffix.is_empty() => (S::None, vec![A::EndComposition, committed(s)]),
        // 候補が入力の一部だけに当たるときは、その部分だけを確定して残りを続ける
        Key::Enter => (
            S::Composing,
            vec![committed(s), A::ShrinkText(String::new())],
        ),
        Key::Escape => (S::None, vec![A::RemoveText, A::EndComposition]),
        Key::Navigation(Navigation::Right) => (S::Composing, vec![A::MoveCursor(1)]),
        Key::Navigation(Navigation::Left) => (S::Composing, vec![A::MoveCursor(-1)]),
        Key::Navigation(Navigation::Up) => {
            (S::Previewing, vec![A::SetSelection(SetSelectionType::Up)])
        }
        Key::Navigation(Navigation::Down) => {
            (S::Previewing, vec![A::SetSelection(SetSelectionType::Down)])
        }
        Key::ToggleInputMode | Key::DirectMode => (
            S::None,
            vec![
                A::EndComposition,
                committed(s),
                A::SetIMEMode(InputMode::Latin),
            ],
        ),
        // 変換キー：段2で並べ替える。変換したあとは次の候補へ
        Key::Space if !previewing => (S::Previewing, vec![A::Convert]),
        Key::Space | Key::Tab => (S::Previewing, vec![A::SetSelection(SetSelectionType::Down)]),
        Key::Function(f) => (
            S::Previewing,
            vec![A::SetTextWithType(match f {
                Function::Six => SetTextType::Hiragana,
                Function::Seven => SetTextType::Katakana,
                Function::Eight => SetTextType::HalfKatakana,
                Function::Nine => SetTextType::FullLatin,
                Function::Ten => SetTextType::HalfLatin,
            })],
        ),
        Key::AiokeyMode | Key::Unknown => return None,
    })
}

/// サーバーの GetComposingText の応答のうち、ライブ変換の判断に使うもの。
pub struct Reply {
    pub revision: u64,
    pub settled: bool,
    pub reranked: bool,
    pub uncertain: bool,
}

/// 段2の結果をライブ変換の表示にどう使うか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveUpdate {
    /// 段2がまだ済んでいない。もう少し待つ
    Wait,
    /// 並べ替えた 1 位で表示を更新する
    Apply,
    /// 確信が低いので、表示は変えずに候補ウィンドウを開く
    OpenCandidates,
    /// 表示は変えない（入力が変わった、または段2を呼ばなかった）
    Keep,
}

pub fn live_update(current_revision: u64, reply: &Reply) -> LiveUpdate {
    if reply.revision != current_revision {
        LiveUpdate::Keep
    } else if !reply.settled {
        LiveUpdate::Wait
    } else if !reply.reranked {
        LiveUpdate::Keep
    } else if reply.uncertain {
        LiveUpdate::OpenCandidates
    } else {
        LiveUpdate::Apply
    }
}
