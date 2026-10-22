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

pub fn key_from_vk(_vk: u32, _shift: bool, _to_char: impl FnOnce() -> Option<char>) -> Key {
    todo!()
}

pub struct Snapshot<'a> {
    pub state: CompositionState,
    pub mode: InputMode,
    pub preview: &'a str,
    pub suffix: &'a str,
    pub context: &'a str,
}

pub fn transition(_s: &Snapshot, _key: Key) -> Option<(CompositionState, Vec<ClientAction>)> {
    todo!()
}

pub struct Reply {
    pub revision: u64,
    pub settled: bool,
    pub reranked: bool,
    pub uncertain: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveUpdate {
    Wait,
    Apply,
    OpenCandidates,
    Keep,
}

pub fn live_update(_current_revision: u64, _reply: &Reply) -> LiveUpdate {
    todo!()
}
