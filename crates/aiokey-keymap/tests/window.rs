//! Issue #12 の受け入れテスト（基準 5・6）：候補ウィンドウを開く時機と、数字キーでの選択。
//!
//! 基準 1〜4（依存、レイアウト、ページ送り、モード表示）は aiokey-ui の tests/ui.rs で確かめる。

use aiokey_keymap::{
    transition, ClientAction, CompositionState, InputMode, Key, SetSelectionType, Snapshot,
};

fn candidates() -> Vec<String> {
    (1..=20).map(|i| format!("候補{i}")).collect()
}

fn snapshot<'a>(state: CompositionState, window_open: bool, list: &'a [String]) -> Snapshot<'a> {
    Snapshot {
        state,
        mode: InputMode::Kana,
        preview: "候補1",
        suffix: "",
        context: "それは",
        window_open,
        selection: 0,
        candidates: list,
    }
}

fn opens(actions: &[ClientAction]) -> bool {
    actions.contains(&ClientAction::ShowCandidateWindow)
}

fn closes(actions: &[ClientAction]) -> bool {
    actions.contains(&ClientAction::HideCandidateWindow)
}

#[test]
fn candidate_window_opens_on_the_second_space_and_closes_on_commit_or_cancel() {
    let list = candidates();
    // 打ち始め・打っている間は開かない
    let none = snapshot(CompositionState::None, false, &list);
    let (_, actions) = transition(&none, Key::Input('k')).unwrap();
    assert!(!opens(&actions), "{actions:?}");
    // 1 回目の Space（変換）では開かない
    let composing = snapshot(CompositionState::Composing, false, &list);
    let (_, actions) = transition(&composing, Key::Space).unwrap();
    assert!(!opens(&actions), "{actions:?}");
    // 2 回目の Space（次の候補へ）で開く
    let previewing = snapshot(CompositionState::Previewing, false, &list);
    let (state, actions) = transition(&previewing, Key::Space).unwrap();
    assert_eq!(state, CompositionState::Previewing);
    assert_eq!(
        actions,
        vec![
            ClientAction::SetSelection(SetSelectionType::Down),
            ClientAction::ShowCandidateWindow
        ]
    );
    // 開いたまま打ち続けたら閉じる
    let open = snapshot(CompositionState::Previewing, true, &list);
    let (state, actions) = transition(&open, Key::Input('k')).unwrap();
    assert_eq!(state, CompositionState::Composing);
    assert!(closes(&actions), "{actions:?}");
    // 確定（Enter）と取り消し（Escape）で閉じる
    for key in [Key::Enter, Key::Escape] {
        let (state, actions) = transition(&open, key).unwrap();
        assert_eq!(state, CompositionState::None, "{key:?}");
        assert!(closes(&actions), "{key:?}: {actions:?}");
    }
}

#[test]
fn number_keys_pick_a_candidate_on_the_shown_page() {
    let list = candidates();
    // 11 番目（2 ページ目）を選んでいるとき、3 は 2 ページ目の 3 番目（12 番目、添字 11）
    let open = Snapshot {
        selection: 10,
        ..snapshot(CompositionState::Previewing, true, &list)
    };
    let (state, actions) = transition(&open, Key::Number(3)).unwrap();
    assert_eq!(state, CompositionState::None);
    assert_eq!(
        actions,
        vec![
            ClientAction::SetSelection(SetSelectionType::Number(11)),
            ClientAction::EndComposition,
            ClientAction::SetContext("それは候補12".into()),
            ClientAction::HideCandidateWindow,
        ]
    );
    // ページにない番号（3 ページ目は 2 件だけ。0 は使わない）は何もしない
    let last_page = Snapshot {
        selection: 18,
        ..snapshot(CompositionState::Previewing, true, &list)
    };
    assert_eq!(
        transition(&last_page, Key::Number(3)),
        Some((CompositionState::Previewing, vec![]))
    );
    assert_eq!(
        transition(&open, Key::Number(0)),
        Some((CompositionState::Previewing, vec![]))
    );
    // ウィンドウを開いていなければ、数字は今までどおり入力になる
    let closed = snapshot(CompositionState::Composing, false, &list);
    assert_eq!(
        transition(&closed, Key::Number(3)),
        Some((
            CompositionState::Composing,
            vec![ClientAction::AppendText("3".into())]
        ))
    );
}
