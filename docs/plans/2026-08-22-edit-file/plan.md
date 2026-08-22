# Edit file: Plan

Delivers `specs/input.md#edit` and the `editor` key in `specs/config.md#key-semantics`.
Supersedes PR #33 (trsxxii) and PR #79 (jorgerojas26).

## Problem

Spotting a two-character typo mid-review costs about thirty seconds. The reviewer memorizes the
path and line, leaves the pane, finds the file, navigates, fixes it, returns, and presses `r`.
`e` is inert on every line that carries no comment, which is most of them. Two outside
contributors hit this independently and opened PR #33 and PR #79.

## Goal

`e` opens the file the cursor names in the reviewer's editor, at the line under the cursor, and
returns them to the same place.

## Definition of Done

- [ ] `e` on an uncommented read-pane line opens the editor at that line. The edit shows in the diff on return.
- [ ] `e` on a commented line still opens the comment editor. `e` in the comments list still edits the highlighted comment.
- [ ] `e` on a navigator file row opens that file at line 1. A directory row does nothing.
- [ ] `e` in a markdown preview opens the previewed file at line 1.
- [ ] `e` on a deletion or a fold opens at the nearest numbered row above.
- [ ] `editor = "code -g {file}:{line}"` substitutes both placeholders. A value naming neither gets the path appended.
- [ ] With no `editor` key, `$EDITOR` alone opens at the line for the vi family, nano, micro, kakoune, emacs, helix, the VS Code family and its forks, Zed, Sublime Text, the JetBrains family, Xcode, Kate, TextMate, BBEdit, and gedit.
- [ ] A graphical editor is given its wait flag, and a reviewer who already set one does not get it twice.
- [ ] An editor reviewr does not know opens the file without a line rather than a guessed flag.
- [ ] With no `editor` key and no `$VISUAL` or `$EDITOR`, `e` names what to set and opens nothing.
- [ ] Returning restores the open file, the cursor, the scroll, the folds, and the footer's expansion.
- [ ] The footer reads `e edit file` on exactly the rows where it works.
- [ ] `--resolve-plugin-config` prints `editor`.

## Out of Scope

- The editor in a herdr split pane. reviewr runs standalone, so the suspend path must exist anyway. `specs/overview.md` Roadmap.
- `e` on the search screen and in the comments list. Both keep their current meaning.
- Guarding an edit made while an agent's turn is open. `specs/input.md#edit` names the consequence.

## Execution Plan

1. [ ] `src/config.rs`: add `editor: Option<String>` beside `github_host`. Register `"editor"` in `KNOWN_KEYS` (line 76 region), parse it at the `github_host` arm (line 419 region), add the accessor and the `to_json` entry. Reject an empty string as an invalid value. Unit tests for parsing, the empty-string rejection, and the JSON round trip.
2. [ ] `src/lib.rs`: extract the inline mode setup at lines 80 to 90 into `enter_terminal_modes(kbd)`, the mirror of `restore_terminal(kbd)` at line 147. Call it from `run()` and from the resume path, so one function owns the mode stack.
3. [ ] `src/app.rs`: add `EditorTarget { path: String, line: u32 }` and `editor_request: Option<EditorTarget>`. Split `start_edit` so it routes to the comment when `target_comment()` finds one and to a new `request_edit_file()` otherwise. `edit_file_target()` reads the cursor: the read pane takes `self.visible[..=self.diff_cursor].iter().rev().find_map(Row::new_no)` and falls back to 1, the navigator takes `current_entry()` at line 1, a preview takes 1. Unit tests per surface, no terminal needed.
4. [ ] `src/lib.rs`: `run_editor` services one request between frames. It resolves the command from `PluginConfig::editor()`, else `$VISUAL`, else `$EDITOR`, substitutes `{file}` and `{line}`, appends the path when the value names neither, spawns through `proc::command`, then re-enters the modes and calls `request_world_refresh(false, false)`.
5. [ ] `src/app.rs` and `src/ui.rs`: add `FooterAction::EditFile` with the label `e edit file`, and push it on the diff-line, preview-line, fold, open-preview, and file-row cases in `footer_actions`. The commented-line case keeps `e edit` as its primary.
6. [ ] `tests/app_flow.rs`: the comment-wins contest on a commented line, `e` in the comments list still reaching the highlighted comment through the split `start_edit`, the directory-row inertia, the footer label per row, and a world refresh after an external edit rebuilding the open diff's content.
7. [ ] `README.md` keybinding table and the config section. `CHANGELOG.md` bullet under `## [Unreleased]`, naming both contributors.

## Likely Files

| file                 | change                                                                    |
| -------------------- | ------------------------------------------------------------------------- |
| `src/config.rs`      | the `editor` key: known-key list, parser, field, accessor, `to_json`      |
| `src/app.rs`         | `EditorTarget`, `edit_file_target`, `start_edit` routing, footer actions  |
| `src/lib.rs`         | `enter_terminal_modes`, `run_editor`, the event-loop service point        |
| `src/ui.rs`          | the `EditFile` footer label                                               |
| `tests/app_flow.rs`  | the contest, the inert rows, the footer, the refresh                      |
| `README.md`          | the keybinding row and the `editor` config section                        |

No change to `src/keymap.rs`. `edit` is one action and stays bound to `e`.

## Verification

- `just ci` → green, run as its cargo steps when `just` is unavailable.
- `cargo test --test app_flow edit` → the contest, the inert rows, and the footer pass.
- A PTY smoke test with a scripted `$EDITOR` → the alternate screen is actually left, the editor's own output reaches the terminal, reviewr repaints, and `q` still quits.
- `just qa-install`, then the user reopens their panes → the demo runs end to end in a real herdr pane.
- `CFG-WHOLE-FILE` -> the empty-string `editor` test in `src/config.rs` -> the whole file is invalid and the pane blocks.
- Tight: everything the diff adds is exercised by a DoD line. Delete or defer the rest.
- Perf bench: `footer_bands` asks `edit_opens_a_file()` twice a frame, so the change is on the render path. Five interleaved A/B rounds against `main`, fixture, painted medians → every scenario inside the run-to-run spread. Baseline unchanged.
- Gate: promote `specs/input.md` and `specs/config.md` to Current.

## Landing

Both PRs are superseded by one branch. Neither contributor's branch is pushed to, since PR #33's
head is the default branch of their own fork.

Credit is prose, never authorship metadata. Neither contributor wrote this implementation, so no
`Co-authored-by` trailer rides its commits. That trailer would claim they authored code they did
not write and would post the commit to their GitHub contribution graph.

| where              | what it says                                                 |
| ------------------ | ------------------------------------------------------------ |
| the commit body    | `Prior art: #33 (@trsxxii), #79 (@jorgerojas26).`            |
| the PR description | what each PR established, with both links                    |
| `CHANGELOG.md`     | the feature bullet, naming both handles                      |
| each closed PR     | a comment naming the design it shaped, linking the merged PR |

The user posts the closing comments, never the agent unprompted. Neither contributor lands on the
repository's contributor graph, because no commit of theirs merges. Nothing available makes that
true without misstating who wrote the code.

## Replan

- If the terminal fails to restore cleanly under a herdr pane, then narrow step 2 to the exact mode subset herdr needs and log which one broke.
- If `{line}` substitution proves wrong for a real editor the user runs, then reopen the spelling fork in brainstorming rather than special-casing that editor.
- 2026-08-22: initial plan.
- 2026-08-22: the user required every major 2026 editor to work, terminal and IDE alike -> the `$EDITOR` fallback grew from one `+{line}` form to a name-keyed table of four argument dialects plus the wait flag graphical editors need -> `src/editor.rs`, `specs/input.md` Edit, `specs/config.md` Key semantics. The `editor` template key stays as the escape hatch, which is what makes a stale table fixable by the reviewer.
- 2026-08-22: the PTY smoke test found `e` opening the scripted editor itself, because the harness wrote it inside the repository under review -> the script moved outside the worktree -> `scripts/smoke_edit_file.py`. No product change.
