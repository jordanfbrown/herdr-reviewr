# Commits scope: Plan

Delivers `specs/review-model.md` Commit pick, `specs/input.md` Commit picker, `specs/tui.md` header paint, and the `rev` comment field.

## Problem

Every scope ends at the worktree. An agent that commits as it goes leaves a ten-commit branch where one commit, or commits 2–5, cannot be read on their own. The nearest workaround is a base pick of `HEAD~3`, which drags uncommitted edits in and cannot stop short of `HEAD`.

## Goal

A fourth scope, `commits`, diffs a picked run of commits `A^..B` from a popup picker. The pick lives in memory, survives a rewrite under it, and comments made on it never land on worktree lines.

## Definition of Done

- [ ] `G` on a file tab opens the picker listing `merge-base..HEAD` newest first, `sha  subject  age` per row, titled `commits · N over <base>`. Without a base it lists the last 50 from `HEAD`, titled `commits · last 50`.
- [ ] `enter` picks the highlighted commit. The scope switches to `commits`, `Changes` lists the files of `git diff A^ B`, the header reads `commits <sha> <subject>`, and both diff sides come from `git show`.
- [ ] `v` anchors, movement draws a bar, the footer reads `enter pick N`, `enter` picks the run. The header reads `commits <a>..<b> (N)`. `esc` clears the anchor, a second `esc` closes.
- [ ] Reopening on a run restores the anchor on the oldest and the highlight on the newest. Reopening on a single commit restores no anchor, so `k` `enter` steps.
- [ ] `g` with a pick switches straight to it. `g`, the chip's `commits` step, and `G` with no pick open the picker without switching. `esc` leaves the previous scope active.
- [ ] `G` and `g` are inert on `PR`, while composing, in the comments list, in another picker, in search, and in find. `/`, `ctrl+f`, `q`, `1`–`3`, and every other key are inert inside the picker. Page keys move the highlight.
- [ ] A pick with a commit unreachable from `HEAD` keeps painting, the header appends `· off branch`, and the picker shows it as one row above the list that takes no anchor. A base change marks nothing.
- [ ] A pick with a pruned commit reads `commit <sha> is gone` in both panes, the header appends `· gone`, row 1 leads with `G pick commits`, and `g` opens the picker.
- [ ] `off branch` and `gone` land with the changeset, from one world build, and a stale build for an old pick is discarded.
- [ ] A poll under the open picker refreshes the list and reconciles the highlight and the anchor by sha.
- [ ] A comment made on a commit diff carries `rev = <B>`. It renders in `Changes` only while the scope's new side is that commit. A worktree comment renders under `uncommitted`, `branch`, and `last-turn`. The comments list and export carry every comment unchanged.
- [ ] `All files` marks the files the run touched and lists the worktree.
- [ ] Config recovery restores an open commit picker with its highlight and anchor.
- [ ] The footer reads `u/b/t/g scope`. `default_scope = "commits"` is a config error.
- [ ] README names the scope and its keys.

## Out of Scope

- Next/prev-commit step keys. Rejected in brainstorming, the picker is the step.
- A commit strip in the navigator. Roadmap candidate, not specced.
- A commit in the export header. Non-goal in `specs/review-model.md`.
- A text filter in the picker. Non-goal in `specs/review-model.md`.
- Passing a commit from the commit picker to the base picker.
- Release packaging.

## Execution Plan

1. [ ] `src/model.rs`: `Scope::Commits` with `label()` `commits`, `name()` `commits`, and `cycle()` `LastTurn → Commits → Uncommitted`. A `CommitPick { oldest: String, newest: String }` type. `Comment.rev: Rev` with `Rev::{Worktree, Commit(String)}`, stamped in `App::build_comment` (`src/app.rs:3373`). Tests beside the enum.

2. [ ] `src/git.rs`: `changed_between(repo, old, new)` runs `git diff <old> <new> --numstat -z` and `--name-status -z` through the existing `assemble` with no untracked pass. `parent_or_empty(repo, sha)` resolves `A^`, the empty tree for a root. `list_commits(repo, base: Option<&str>)` runs `git log --format=%H%x00%s%x00%ct` over `merge-base..HEAD`, or `-50 HEAD`. `is_reachable(repo, sha)` via `git merge-base --is-ancestor`. `commit_exists(repo, sha)` via `cat-file -e`. Tests in `tests/git_repo.rs`: a run of three, a root commit, a merge commit, a rewritten sha still diffs, a pruned sha is reported missing.

3. [ ] `src/world.rs`: `WorldInput.commit_pick: Option<CommitPick>` joins the identity tag. `build_changed` gains the `Commits` arm and returns the pick's verdict (`Live`, `OffBranch`, `Gone(sha)`) in `WorldSnapshot`. `src/app.rs` `reconcile_world` adopts the verdict only while `scope == Commits`, mirroring `adopt_branch_base` (`src/app.rs:1160`). `content_sides` (`src/app.rs:1442`) gains the arm reading both sides with `file_content`. Test in `tests/app_flow.rs`: a stale build for a replaced pick is discarded, the verdict lands with the changeset.

4. [ ] `src/app.rs`: `Mode::CommitPick` and `CommitPicker { rows, cursor, anchor: Option<usize>, pick_row: Option<CommitPick> }` modeled on `BasePicker` (`src/app.rs:129`). `open_commit_picker` lists rows, inserts the pick row when the pick is not wholly listed, places the highlight and anchor per the spec. `commit_picker_move`, `commit_picker_page`, `commit_picker_anchor`, `commit_picker_pick`, `close_commit_picker`. `set_scope(Commits)` with no or a gone pick opens the picker. The chip's cycle uses the same path. A poll refreshes rows and reconciles cursor and anchor by sha. `comment_in_view` (`src/app.rs:3417`) adds the `rev` match. `commits_gone()` and its message mirror `awaiting_turn` (`src/app.rs:1474`). Tests in `tests/app_flow.rs`: open on each scope, single pick, run pick both directions, reopen with and without anchor, `esc` twice, inert keys, off-branch row, gone pick reopens, rev-gated rendering, poll reconciliation, config recovery.

5. [ ] `src/keymap.rs`: `Action::ScopeCommits` (`scope-commits`, `g`) and `Action::CommitPick` (`commit-pick`, `G`), array length bumped. `src/lib.rs`: the `Mode::CommitPick` key block before the tab handlers with an inert catch-all like `Mode::Picker` (`src/lib.rs:1749`), the two dispatch arms beside `K::BasePick` (`src/lib.rs:1885`), popup click routing beside the base picker's (`src/lib.rs:2451`), and `HeaderHit::Pick` opening the picker. Keymap tests: the defaults bind both, `g` and `G` collide with nothing.

6. [ ] `src/ui.rs`: `render_commit_picker` and `commit_picker_popup` beside `render_base_picker` (`src/ui.rs:3044`), with the bar column, the pick row, and the empty states. `hit_commit_picker_row`. Header: `pick_label` / `pick_parts` painting `commits <sha> <subject>`, `<a>..<b> (N)`, `· off branch`, `· gone`, subject truncation keeping sha and marker. `HeaderHit::Pick`. `action_key_label` `A::Scope` adds the fourth hint, `A::ScopeOther` drops the active one of four. `footer_bands` (`src/app.rs:3875`): the gone row leads with `CommitPick`, the `go` band carries `CommitPick` on file tabs outside row 1, the picker's own one-row footer with `enter pick N`. The two empty-state sites (`src/ui.rs:1490`, `:1776`) gain the gone message. Tests in `tests/render.rs`: picker paint with a bar, header paints, truncation, footer rows, scrim.

7. [ ] `src/config.rs`: `default_scope` error text lists three values. `README.md`: the scope, `g`/`G`, the `v` range.

8. [ ] Tests in the same commits as the code they check. `just ci`. QA: `just qa-install`, pick one commit, a run, rebase under it, confirm the header marker and that `k` `enter` steps.

## Likely Files

| file                 | change                                                        |
| -------------------- | ------------------------------------------------------------- |
| `src/model.rs`       | `Scope::Commits`, `CommitPick`, `Comment.rev`                 |
| `src/git.rs`         | two-rev diff, commit list, reachability, existence            |
| `src/world.rs`       | input tag, `Commits` build arm, verdict                       |
| `src/app.rs`         | `Mode::CommitPick`, picker state and verbs, rev gate, gone    |
| `src/keymap.rs`      | `scope-commits`, `commit-pick`                                |
| `src/lib.rs`         | picker keys, dispatch, clicks                                 |
| `src/ui.rs`          | picker paint, header pick label, footer, empty states         |
| `src/config.rs`      | `default_scope` error text                                    |
| `README.md`          | the scope and its keys                                        |
| `tests/git_repo.rs`  | two-rev diffs, root, merge, pruned                            |
| `tests/app_flow.rs`  | picker flow, rev gate, world tag, recovery                    |
| `tests/render.rs`    | picker and header paint, footer rows                          |

## Verification

- `cargo test --test git_repo` → a run of three diffs `A^..B`, a root commit diffs against the empty tree, a pruned sha reports missing.
- `cargo test --test app_flow` → single and run picks switch scope, reopen restores per spec, `esc` twice, inert keys, off-branch and gone states, a stale build is discarded, a commit comment hides under `uncommitted`.
- `cargo test --test render` → picker with a bar and `enter pick 3`, header `commits a..b (4)`, `· off branch`, `· gone`, footer `u/b/t/g scope` and `G pick commits`.
- No writes: the git test asserts no ref, index, or worktree change after a pick, only the existing `refs/reviewr/` refs.
- `python3 scripts/bench_tui.py --binary target/release/herdr-reviewr --fixture` before and after on a quiet system, medians within noise. The new code is off the hot paths, the bench confirms it.
- `just ci` green.
- Tight: everything the diff adds is exercised by a DoD line.
- [ ] Gate: promote `specs/review-model.md`, `specs/input.md`, `specs/tui.md`, `specs/diff-view.md`, `specs/search.md`, `specs/find-in-file.md`, `specs/overview.md` to Current.

## Replan

- If `git diff A^ B` rename detection differs from the worktree diffs in a way `assemble` cannot absorb, add `-M` explicitly and note it in `specs/review-model.md`.
- If the poll-time reachability check for `off branch` costs more than one `merge-base --is-ancestor` per poll shows on the bench, compute it inside the world build only, never on the paint path.
- 2026-08-23: initial plan.
