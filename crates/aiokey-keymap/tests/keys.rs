//! Issue #11 の受け入れテスト：TSF のキー処理を AI-O Key の入力規則に合わせる。
//!
//! 基準 8 のうちサーバーが margin を返す部分は、aiokey-server の tests/conversion.rs で確かめる。

use aiokey_keymap::{
    key_from_vk, live_update, transition, ClientAction, CompositionState, InputMode, Key,
    LiveUpdate, Reply, SetSelectionType, Snapshot,
};

const VK_SPACE: u32 = 0x20;
const VK_OEM_MINUS: u32 = 0xBD;
const VK_OEM_2: u32 = 0xBF; // `/`
const VK_DBE_ALPHANUMERIC: u32 = 0xF0; // 英数
const VK_DBE_HIRAGANA: u32 = 0xF2; // カタカナ／ひらがな
const VK_IME_ON: u32 = 0x16;
const VK_IME_OFF: u32 = 0x1A;
const VK_DBE_SBCSCHAR: u32 = 0xF3; // 半角／全角
const VK_DBE_DBCSCHAR: u32 = 0xF4;

fn snapshot(state: CompositionState, mode: InputMode) -> Snapshot<'static> {
    Snapshot {
        state,
        mode,
        preview: "",
        suffix: "",
        context: "",
        window_open: false,
        selection: 0,
        candidates: &[],
    }
}

fn append(s: &str) -> ClientAction {
    ClientAction::AppendText(s.to_string())
}

#[test]
fn letters_and_hyphen_are_input_in_kana_mode() {
    let keys = ('a'..='z').map(|c| (0x41 + (c as u32 - 'a' as u32), c));
    for (vk, c) in keys.chain([(VK_OEM_MINUS, '-')]) {
        let key = key_from_vk(vk, false, || Some(c));
        assert_eq!(key, Key::Input(c));
        let none = snapshot(CompositionState::None, InputMode::Kana);
        assert_eq!(
            transition(&none, key),
            Some((
                CompositionState::Composing,
                vec![ClientAction::StartComposition, append(&c.to_string())]
            )),
            "{c}"
        );
        let composing = snapshot(CompositionState::Composing, InputMode::Kana);
        assert_eq!(
            transition(&composing, key),
            Some((CompositionState::Composing, vec![append(&c.to_string())])),
            "{c}"
        );
    }
}

#[test]
fn shift_space_adds_a_separator() {
    assert_eq!(key_from_vk(VK_SPACE, true, || Some(' ')), Key::Separator);
    assert_eq!(key_from_vk(VK_SPACE, false, || Some(' ')), Key::Space);
    for state in [CompositionState::Composing, CompositionState::Previewing] {
        assert_eq!(
            transition(&snapshot(state, InputMode::Kana), Key::Separator),
            Some((CompositionState::Composing, vec![append(" ")])),
            "{state:?}"
        );
    }
}

#[test]
fn slash_starts_an_english_word() {
    let key = key_from_vk(VK_OEM_2, false, || Some('/'));
    assert_eq!(key, Key::Input('/'));
    assert_eq!(
        transition(&snapshot(CompositionState::None, InputMode::Kana), key),
        Some((
            CompositionState::Composing,
            vec![ClientAction::StartComposition, append("/")]
        ))
    );
    // 続く英字（大文字も）はそのまま送る
    let upper = key_from_vk(0x48, true, || Some('H'));
    assert_eq!(
        transition(
            &snapshot(CompositionState::Composing, InputMode::Kana),
            upper
        ),
        Some((CompositionState::Composing, vec![append("H")]))
    );
}

#[test]
fn eisu_kana_and_hankaku_zenkaku_switch_modes() {
    assert_eq!(
        key_from_vk(VK_DBE_ALPHANUMERIC, false, || None),
        Key::DirectMode
    );
    assert_eq!(key_from_vk(VK_IME_OFF, false, || None), Key::DirectMode);
    assert_eq!(
        key_from_vk(VK_DBE_HIRAGANA, false, || None),
        Key::AiokeyMode
    );
    assert_eq!(key_from_vk(VK_IME_ON, false, || None), Key::AiokeyMode);
    assert_eq!(
        key_from_vk(VK_DBE_SBCSCHAR, false, || None),
        Key::ToggleInputMode
    );
    assert_eq!(
        key_from_vk(VK_DBE_DBCSCHAR, false, || None),
        Key::ToggleInputMode
    );

    let set = |m| ClientAction::SetIMEMode(m);
    let kana = snapshot(CompositionState::None, InputMode::Kana);
    let latin = snapshot(CompositionState::None, InputMode::Latin);
    assert_eq!(
        transition(&kana, Key::DirectMode),
        Some((CompositionState::None, vec![set(InputMode::Latin)]))
    );
    assert_eq!(
        transition(&latin, Key::AiokeyMode),
        Some((CompositionState::None, vec![set(InputMode::Kana)]))
    );
    assert_eq!(
        transition(&latin, Key::ToggleInputMode),
        Some((CompositionState::None, vec![set(InputMode::Kana)]))
    );
    assert_eq!(
        transition(&kana, Key::ToggleInputMode),
        Some((CompositionState::None, vec![set(InputMode::Latin)]))
    );
    // 直接入力では英字を IME で受けない
    assert_eq!(transition(&latin, Key::Input('a')), None);
    // 変換中に英数キーを押したら、確定してから直接入力にする
    let composing = Snapshot {
        preview: "果たして",
        ..snapshot(CompositionState::Composing, InputMode::Kana)
    };
    let (state, actions) = transition(&composing, Key::DirectMode).unwrap();
    assert_eq!(state, CompositionState::None);
    assert_eq!(actions.first(), Some(&ClientAction::EndComposition));
    assert_eq!(actions.last(), Some(&set(InputMode::Latin)));
}

#[test]
fn committed_text_is_sent_as_context() {
    let s = Snapshot {
        preview: "果たして本当に",
        context: "それは",
        ..snapshot(CompositionState::Previewing, InputMode::Kana)
    };
    let (state, actions) = transition(&s, Key::Enter).unwrap();
    assert_eq!(state, CompositionState::None);
    assert_eq!(
        actions,
        vec![
            ClientAction::EndComposition,
            ClientAction::SetContext("それは果たして本当に".into())
        ]
    );
    // 文脈は後ろの 40 文字まで（段2の採点器が使う長さ）
    let long = "あ".repeat(50);
    let s = Snapshot {
        preview: "い",
        context: &long,
        ..snapshot(CompositionState::Composing, InputMode::Kana)
    };
    let (_, actions) = transition(&s, Key::Enter).unwrap();
    assert_eq!(
        actions.last(),
        Some(&ClientAction::SetContext(format!("{}い", "あ".repeat(39))))
    );
}

#[test]
fn space_converts_then_moves_to_the_next_candidate() {
    assert_eq!(
        transition(
            &snapshot(CompositionState::Composing, InputMode::Kana),
            Key::Space
        ),
        Some((CompositionState::Previewing, vec![ClientAction::Convert]))
    );
    // 候補ウィンドウを開いたあとの Space（ウィンドウを開く時機は #12 の tests/window.rs）
    let open = Snapshot {
        window_open: true,
        ..snapshot(CompositionState::Previewing, InputMode::Kana)
    };
    assert_eq!(
        transition(&open, Key::Space),
        Some((
            CompositionState::Previewing,
            vec![ClientAction::SetSelection(SetSelectionType::Down)]
        ))
    );
}

#[test]
fn stage2_result_updates_the_live_conversion_only_for_the_same_input() {
    let reply = |revision, settled, reranked, uncertain| Reply {
        revision,
        settled,
        reranked,
        uncertain,
    };
    // 段2がまだ済んでいなければ待つ
    assert_eq!(
        live_update(5, &reply(5, false, false, false)),
        LiveUpdate::Wait
    );
    // 済んで並べ替えたら、表示を更新する
    assert_eq!(
        live_update(5, &reply(5, true, true, false)),
        LiveUpdate::Apply
    );
    // 入力が変わっていたら（古い結果）、表示は変えない
    assert_eq!(
        live_update(6, &reply(5, true, true, false)),
        LiveUpdate::Keep
    );
    // 段2を呼ばなかった（段1の確信度が高い）ときも、表示は変えない
    assert_eq!(
        live_update(5, &reply(5, true, false, false)),
        LiveUpdate::Keep
    );
    // 基準 8：margin が閾値未満なら、表示を更新せずに候補ウィンドウを開く
    assert_eq!(
        live_update(5, &reply(5, true, true, true)),
        LiveUpdate::OpenCandidates
    );
}
