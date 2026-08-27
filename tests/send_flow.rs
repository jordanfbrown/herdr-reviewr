//! End-to-end send dispatch through a fake herdr binary (`specs/herdr-host.md`,
//! `specs/input.md`). This file is its own test process, so the HERDR_* environment it
//! sets can never leak into another test binary, and no real herdr pane is ever addressed.
#![cfg(unix)]

mod common;

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Repo, app_on};
use herdr_reviewr::app::{App, Focus, FooterAction, Mode, Tab};
use herdr_reviewr::keymap::Keymap;
use herdr_reviewr::ui;
use herdr_reviewr::{handle_key, handle_mouse};
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

// `cwd` rides every real `agent list` entry (api notes). Send ignores it and resolves from
// the workspace, so it is here to keep the fixture honest rather than to steer the send.
const TWO_AGENTS: &str = r#"{"result":{"agents":[
  {"agent":"claude","agent_status":"idle","pane_id":"w8:p1","tab_id":"w8:t1","workspace_id":"w8","cwd":"/w/one"},
  {"agent":"codex","agent_status":"working","pane_id":"w8:p2","tab_id":"w8:t1","workspace_id":"w8","cwd":"/w/two"}
]}}"#;
const ONE_AGENT: &str = r#"{"result":{"agents":[
  {"agent":"claude","agent_status":"idle","pane_id":"w8:p1","tab_id":"w8:t1","workspace_id":"w8","cwd":"/w/one"}
]}}"#;

/// A fake herdr: answers `agent list` from `agents.json`, `tab list` with one label, logs
/// every invocation, and succeeds at everything else (`pane send-text`, `pane focus`). It
/// fails whatever `fail` holds, so a dead pane and a broken enumeration both have a shape.
fn write_fake_herdr(dir: &Path) -> PathBuf {
    let script = dir.join("herdr");
    fs::write(
        &script,
        "#!/bin/sh\n\
         dir=$(dirname \"$0\")\n\
         echo \"$@\" >> \"$dir/log\"\n\
         case \"$*\" in\n\
           $(cat \"$dir/fail\" 2>/dev/null || echo __none__)*)\n\
             echo '{\"error\":{\"code\":\"pane_not_found\",\"message\":\"pane w8:p1 not found\"},\"id\":\"cli:request\"}' >&2\n\
             exit 1 ;;\n\
         esac\n\
         case \"$1 $2\" in\n\
           \"agent list\") cat \"$dir/agents.json\" ;;\n\
           \"tab list\") echo '{\"result\":{\"tabs\":[{\"tab_id\":\"w8:t1\",\"label\":\"Grip\"}]}}' ;;\n\
           *) : ;;\n\
         esac\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    script
}

/// Make the fake herdr exit non-zero for every invocation starting with `prefix`.
fn fail_on(dir: &Path, prefix: &str) {
    fs::write(dir.join("fail"), prefix).unwrap();
}

fn fail_on_nothing(dir: &Path) {
    let _ = fs::remove_file(dir.join("fail"));
}

fn log(dir: &Path) -> String {
    fs::read_to_string(dir.join("log")).unwrap_or_default()
}

/// Save one comment on the first added line, so `Send` has something to deliver.
fn write_comment(app: &mut App, text: &str) {
    app.focus = Focus::Diff;
    app.diff_cursor = app.visible.iter().position(|r| r.marker() == '+').unwrap();
    app.start_comment();
    app.input = text.to_string();
    app.submit_comment();
}

fn press(app: &mut App, code: KeyCode, area: Rect, keymap: &Keymap) {
    handle_key(app, KeyEvent::from(code), area, keymap).unwrap();
}

/// The crate forbids `unsafe`, which rules out in-process `env::set_var`, so the parent
/// run re-executes this same test in a child process with the HERDR_* seam applied at
/// spawn — env applied to a child is safe, and the child alone runs the body.
#[test]
fn send_dispatches_one_agent_directly_and_several_through_the_picker() {
    if env::var("SEND_FLOW_CHILD").is_err() {
        let staging = tempfile::TempDir::new().expect("tempdir");
        let script = write_fake_herdr(staging.path());
        let out = std::process::Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "send_dispatches_one_agent_directly_and_several_through_the_picker",
                "--nocapture",
            ])
            .env("SEND_FLOW_CHILD", "1")
            .env("FAKE_HERDR_DIR", staging.path())
            .env("HERDR_BIN_PATH", &script)
            .env("HERDR_WORKSPACE_ID", "w8")
            .env("HERDR_PANE_ID", "w8:p9")
            .output()
            .expect("re-exec the test with the fake herdr env");
        assert!(
            out.status.success(),
            "child run failed:\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        // libtest exits 0 when `--exact` matches nothing, so the status alone cannot tell a
        // passing body from a filter that selected no test. The fake herdr's log is the proof
        // the body actually ran and delivered.
        assert!(
            log(staging.path()).contains("pane send-text"),
            "the child ran no send — did the test name and the `--exact` filter drift apart?\n{}",
            String::from_utf8_lossy(&out.stdout),
        );
        return;
    }

    let r = Repo::init();
    r.write("a.rs", "alpha\n");
    r.commit_all("init");
    r.write("a.rs", "alpha\nbeta\n");

    let fake_dir = PathBuf::from(env::var("FAKE_HERDR_DIR").expect("set by the parent run"));
    let keymap = Keymap::default();
    let area = Rect::new(0, 0, 80, 24);
    let mut app = app_on(&r);

    // Several agents: `s` opens the picker over both rows, labelled from `tab list`, and with
    // nothing sent yet the highlight arms on the first row (`specs/herdr-host.md`).
    fs::write(fake_dir.join("agents.json"), TWO_AGENTS).unwrap();
    write_comment(&mut app, "one");
    press(&mut app, KeyCode::Char('s'), area, &keymap);
    assert_eq!(app.mode, Mode::Picker, "several agents open the picker");
    assert_eq!(app.picker_rows.len(), 2);
    assert_eq!(app.picker_rows[0].tab, "Grip", "the tab label joins on tab_id");
    assert_eq!(app.picker_cursor, 0, "nothing sent this session arms the first row");

    // A chosen pane that closed while the picker was open fails the send, and every comment
    // stays. Nothing arms, since nothing was delivered (specs/herdr-host.md).
    fail_on(&fake_dir, "pane send-text w8:p1");
    press(&mut app, KeyCode::Enter, area, &keymap);
    assert_eq!(app.mode, Mode::Normal, "the picker closes whatever the outcome");
    assert_eq!(app.store.len(), 1, "a failed send keeps every comment");
    // One short sentence a reviewer can read. herdr's own wording is a JSON envelope around a
    // pane id, and the argv it came from carries the whole review in its last argument — both
    // would fill a 40-column footer without naming anything (`specs/herdr-host.md`).
    assert_eq!(app.status, "agent not found");
    assert_eq!(app.last_sent_pane, None, "a failed send arms nothing");
    fail_on_nothing(&fake_dir);

    // One agent: `s` sends straight through, no picker frame in between — and the direct
    // send arms its agent like a picker send.
    fs::write(fake_dir.join("agents.json"), ONE_AGENT).unwrap();
    press(&mut app, KeyCode::Char('s'), area, &keymap);
    assert_eq!(app.mode, Mode::Normal, "one agent sends directly");
    assert!(app.store.is_empty(), "a successful send consumes the whole set");
    assert_eq!(app.status, "added 1 comment to claude");
    assert_eq!(app.last_sent_pane.as_deref(), Some("w8:p1"));

    // `enter` sends to the digit-selected agent and consumes the set.
    fs::write(fake_dir.join("agents.json"), TWO_AGENTS).unwrap();
    write_comment(&mut app, "two");
    press(&mut app, KeyCode::Char('s'), area, &keymap);
    press(&mut app, KeyCode::Char('2'), area, &keymap);
    press(&mut app, KeyCode::Enter, area, &keymap);
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.store.is_empty(), "a successful send consumes the whole set");
    assert_eq!(app.status, "added 1 comment to codex");
    assert_eq!(app.last_sent_pane.as_deref(), Some("w8:p2"));
    assert!(log(&fake_dir).contains("pane send-text w8:p2"), "log: {}", log(&fake_dir));
    // The start marker opens the payload at the CLI boundary; `pasted()` owns the rationale.
    assert!(
        log(&fake_dir).contains("pane send-text w8:p2 \u{1b}[200~"),
        "the send is framed as a bracketed paste: {}",
        log(&fake_dir)
    );
    // The batch's last bytes are the comment text "two", so this pins the terminator to the
    // end of a delivered payload.
    assert!(
        log(&fake_dir).contains("two\u{1b}[201~"),
        "the frame terminator closes the batch: {}",
        log(&fake_dir)
    );
    assert!(log(&fake_dir).contains("agent focus w8:p2"), "a send focuses its pane");

    // Several again: the last-sent agent outranks the first row, and a first click on that
    // armed row sends immediately (specs/input.md).
    write_comment(&mut app, "three");
    press(&mut app, KeyCode::Char('s'), area, &keymap);
    assert_eq!(app.mode, Mode::Picker);
    assert_eq!(app.picker_cursor, 1, "the last-sent agent outranks the first row");
    let (col, row) = (0..area.height)
        .flat_map(|y| (0..area.width).map(move |x| (x, y)))
        .find(|&(x, y)| ui::hit_picker_row(area, &app, x, y) == Some(1))
        .expect("the armed row is clickable");
    handle_mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
        &[],
        &keymap,
        &herdr_reviewr::export::Clipboard,
    )
    .unwrap();
    assert_eq!(app.mode, Mode::Normal, "a first click on the armed row sends");
    assert!(app.store.is_empty());
    let sends = log(&fake_dir).matches("pane send-text w8:p2").count();
    assert_eq!(
        sends,
        2,
        "the digit-selected send and the armed-row click addressed the same pane: {}",
        log(&fake_dir)
    );

    // No agent, and an enumeration herdr never answered, both refuse and name the clipboard —
    // and neither opens a picker.
    fs::write(fake_dir.join("agents.json"), r#"{"result":{"agents":[]}}"#).unwrap();
    write_comment(&mut app, "four");
    press(&mut app, KeyCode::Char('s'), area, &keymap);
    assert_eq!(app.mode, Mode::Normal, "an empty workspace opens no picker");
    assert_eq!(app.store.len(), 1, "a refusal keeps every comment");
    assert_eq!(app.status, "no agent here — copy to the clipboard instead");

    fail_on(&fake_dir, "agent list");
    press(&mut app, KeyCode::Char('s'), area, &keymap);
    assert_eq!(app.mode, Mode::Normal, "a failed enumeration opens no picker");
    assert_eq!(app.store.len(), 1, "a refusal keeps every comment");
    // A failed enumeration says so rather than claiming a count. The argv and herdr's stderr go
    // to the log, so the sentence still fits a 40-column footer (`specs/input.md`).
    assert_eq!(app.status, "herdr did not answer — copy to the clipboard instead");
}

/// PR sends are a separate process so their fake-herdr environment cannot escape to another
/// integration-test binary.
#[test]
fn pr_send_freezes_the_selected_conversation_across_routing() {
    if env::var("PR_SEND_FLOW_CHILD").is_err() {
        let staging = tempfile::TempDir::new().expect("tempdir");
        let script = write_fake_herdr(staging.path());
        let out = std::process::Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "pr_send_freezes_the_selected_conversation_across_routing",
                "--nocapture",
            ])
            .env("PR_SEND_FLOW_CHILD", "1")
            .env("FAKE_HERDR_DIR", staging.path())
            .env("HERDR_BIN_PATH", &script)
            .env("HERDR_WORKSPACE_ID", "w8")
            .env("HERDR_PANE_ID", "w8:p9")
            .output()
            .expect("re-exec the test with the fake herdr env");
        assert!(
            out.status.success(),
            "child run failed:\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            log(staging.path()).contains("pane send-text"),
            "the child ran no PR send — did the `--exact` filter drift apart?\n{}",
            String::from_utf8_lossy(&out.stdout)
        );
        return;
    }

    use herdr_reviewr::forge::{CommentKind, PrSnapshot, PrView, ThreadMessage};

    let r = Repo::init();
    r.write("a.rs", "alpha\n");
    r.commit_all("init");
    r.write("a.rs", "alpha\nbeta\n");
    let fake_dir = PathBuf::from(env::var("FAKE_HERDR_DIR").expect("set by the parent run"));
    let keymap = Keymap::default();
    let area = Rect::new(0, 0, 80, 24);
    let mut app = app_on(&r);
    write_comment(&mut app, "authored comment");

    let finding = |body: &str| herdr_reviewr::forge::Comment {
        kind: CommentKind::Finding,
        author: "review-bot".into(),
        anchor: "a.rs:2".into(),
        body: body.into(),
        snippet: Some("+beta".into()),
        messages: vec![
            ThreadMessage {
                author: "review-bot".into(),
                author_is_bot: true,
                body: body.into(),
                created_at: "2026-06-27T10:00:00Z".into(),
            },
            ThreadMessage {
                author: "sam".into(),
                author_is_bot: false,
                body: "I will fix it".into(),
                created_at: "2026-06-27T10:01:00Z".into(),
            },
        ],
        conversation_truncated: true,
        ..common::comment()
    };
    let snapshot = |comment| {
        PrView::Pr(Box::new(PrSnapshot {
            number: 42,
            title: "Keep the exact conversation".into(),
            url: "https://forge.example/pr/42".into(),
            body: "A description makes its own inert row.".into(),
            comments: vec![comment],
            ..common::pr_snapshot()
        }))
    };
    let original = finding("Please handle the edge case");
    let payload = "Pull request #42: Keep the exact conversation\nURL: https://forge.example/pr/42\n\nSelected finding by @review-bot\nAnchor: a.rs:2\n\nDiff snippet:\n+beta\n\nConversation:\n\n@review-bot:\nPlease handle the edge case\n\n@sam:\nI will fix it\n\nWarning: this conversation is truncated; open the PR URL for the remaining messages.";
    app.set_tab(Tab::Pr).unwrap();
    app.apply_pr(snapshot(original));
    assert!(app.pr_on_description());
    assert!(
        !app.footer_bands().iter().any(|&(action, _)| action == FooterAction::Send),
        "the description never advertises a send"
    );
    let before_inert = app.status.clone();
    press(&mut app, KeyCode::Char('s'), area, &keymap);
    assert_eq!(app.status, before_inert, "send is inert on the description");
    assert!(!log(&fake_dir).contains("pane send-text"));

    app.pr_move(1);
    assert!(app.pr_selected_comment().is_some());
    assert!(
        app.footer_bands().iter().any(|&(action, _)| action == FooterAction::Send),
        "a selected conversation advertises send"
    );

    // One candidate skips the picker and must deliver a fully framed, self-contained prompt.
    fs::write(fake_dir.join("agents.json"), ONE_AGENT).unwrap();
    press(&mut app, KeyCode::Char('s'), area, &keymap);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.status, "added conversation from @review-bot to claude");
    assert_eq!(app.last_sent_pane.as_deref(), Some("w8:p1"));
    assert_eq!(app.store.len(), 1, "PR sends never consume authored comments");
    let direct = format!("pane send-text w8:p1 \u{1b}[200~{payload}\u{1b}[201~");
    assert!(log(&fake_dir).contains(&direct), "exact direct payload:\n{}", log(&fake_dir));
    assert!(log(&fake_dir).contains("agent focus w8:p1"), "a successful PR send focuses its pane");

    // Several candidates freeze the bytes before the picker opens. A refresh must not replace
    // the selected conversation while the route is still undecided.
    fs::write(fake_dir.join("agents.json"), TWO_AGENTS).unwrap();
    press(&mut app, KeyCode::Char('s'), area, &keymap);
    assert_eq!(app.mode, Mode::Picker);
    app.apply_pr(snapshot(finding("REFRESHED: must not be sent")));
    press(&mut app, KeyCode::Enter, area, &keymap);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.status, "added conversation from @review-bot to claude");
    assert_eq!(app.store.len(), 1, "the frozen PR send preserves authored comments");
    assert_eq!(
        log(&fake_dir).matches(&direct).count(),
        2,
        "the picker sends frozen original bytes: {}",
        log(&fake_dir)
    );
    // Picker tabs are modal/inert, so they cannot replace the frozen source before selection.
    app.send_pr_to_agent();
    assert_eq!(app.mode, Mode::Picker);
    press(&mut app, KeyCode::Char('1'), area, &keymap);
    assert_eq!(app.tab, Tab::Pr, "a tab binding is inert while the picker is open");
    press(&mut app, KeyCode::Esc, area, &keymap);
    assert_eq!(app.mode, Mode::Normal);
    assert!(!log(&fake_dir).contains("REFRESHED: must not be sent"));

    // Escape discards the frozen payload without dispatching it.
    app.set_tab(Tab::Pr).unwrap();
    app.pr_move(1);
    app.send_pr_to_agent();
    assert_eq!(app.mode, Mode::Picker);
    let sends_before_cancel = log(&fake_dir).matches("pane send-text").count();
    press(&mut app, KeyCode::Esc, area, &keymap);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(
        log(&fake_dir).matches("pane send-text").count(),
        sends_before_cancel,
        "cancel sends nothing"
    );
    assert_eq!(app.store.len(), 1);

    // A chosen pane disappearing fails only this send and keeps both independent stores intact.
    app.send_pr_to_agent();
    assert_eq!(app.mode, Mode::Picker);
    fail_on(&fake_dir, "pane send-text w8:p1");
    press(&mut app, KeyCode::Enter, area, &keymap);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.status, "agent not found");
    assert_eq!(app.store.len(), 1, "failed PR send preserves authored comments");
    assert_eq!(
        app.pr_selected_comment().map(|comment| comment.body.as_str()),
        Some("REFRESHED: must not be sent"),
        "failed PR send preserves the forge snapshot"
    );
    fail_on_nothing(&fake_dir);

    fs::write(fake_dir.join("agents.json"), r#"{"result":{"agents":[]}}"#).unwrap();
    app.send_pr_to_agent();
    assert_eq!(app.mode, Mode::Normal, "no agents open no picker");
    assert_eq!(app.status, "no agent here — copy to the clipboard instead");
    assert_eq!(app.store.len(), 1, "no-agent PR send preserves authored comments");
}
