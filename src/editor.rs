//! Resolving the editor command for `edit` on a file (`specs/input.md` Edit).
//!
//! Two sources, in order: the `editor` config key, whose value is a template the user owns
//! outright, and `$VISUAL`/`$EDITOR`, whose binary name selects the argument dialect below.
//! This module is process-free: it builds an argv, and `src/lib.rs` spawns it.

use std::path::Path;

/// How an editor spells "open this file at this line".
///
/// Four shapes cover every editor in [`DIALECTS`]. Sources: lazygit's editor presets, Julia's
/// `InteractiveUtils` editor table, and each vendor's own CLI documentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LineArg {
    /// `+42 <path>` — the vi family, nano, micro, kakoune, emacs, `BBEdit`, gedit.
    Plus,
    /// `<path>:42` — helix, Zed, Sublime Text.
    Suffix,
    /// `-g <path>:42` — the VS Code family.
    Goto,
    /// `--line 42 <path>` — the `JetBrains` family, Xcode, Kate, `TextMate`.
    Flag,
}

/// One editor family: the binary names that select it, how it takes a line, and the flag that
/// makes it block until the file closes.
///
/// A graphical editor returns the moment it hands the file to a running instance, so without
/// its wait flag reviewr would repaint before the reviewer typed anything. A terminal editor
/// owns the pane until it exits and needs none.
struct Dialect {
    names: &'static [&'static str],
    line: LineArg,
    /// The flag that makes it block, `None` when it blocks already. Both spellings, so a
    /// reviewer who already set the short one does not get the long one on top.
    wait: Option<Wait>,
}

/// A wait flag and its short spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Wait {
    long: &'static str,
    short: &'static str,
}

const WAIT: Option<Wait> = Some(Wait { long: "--wait", short: "-w" });
const BLOCK: Option<Wait> = Some(Wait { long: "--block", short: "-b" });
/// `MacVim` and `gVim` take vim's own foreground flag, which has no long spelling.
const FOREGROUND: Option<Wait> = Some(Wait { long: "-f", short: "-f" });

const DIALECTS: &[Dialect] = &[
    // Terminal editors. Each holds the pane until it exits, so none takes a wait flag.
    Dialect {
        names: &["vi", "vim", "nvim", "lvim", "vis", "joe"],
        line: LineArg::Plus,
        wait: None,
    },
    Dialect { names: &["nano", "micro", "kak"], line: LineArg::Plus, wait: None },
    // Emacs is whichever its build and `DISPLAY` make it, and neither is readable from here.
    // The pane is the survivable guess: a windowed Emacs handed the pane leaves the pane blank
    // until it quits, where a terminal Emacs denied it is invisible.
    Dialect { names: &["emacs", "emacsclient"], line: LineArg::Plus, wait: None },
    Dialect { names: &["hx", "helix"], line: LineArg::Suffix, wait: None },
    // MacVim opens a window and returns, so it waits under vim's own flag rather than a GUI one.
    Dialect { names: &["mvim", "gvim"], line: LineArg::Plus, wait: FOREGROUND },
    // Graphical editors.
    Dialect {
        names: &["code", "code-insiders", "codium", "vscodium", "cursor", "windsurf", "positron"],
        line: LineArg::Goto,
        wait: WAIT,
    },
    Dialect { names: &["subl", "sublime_text"], line: LineArg::Suffix, wait: WAIT },
    Dialect { names: &["zed"], line: LineArg::Suffix, wait: WAIT },
    Dialect { names: &["bbedit", "gedit"], line: LineArg::Plus, wait: WAIT },
    Dialect { names: &["mate"], line: LineArg::Flag, wait: WAIT },
    // `xed` names two editors. On macOS it is Xcode's opener, which takes `--line`. On Linux it
    // is Mint's X-Apps editor, a gedit fork that takes `+LINE` and rejects `--line` outright.
    #[cfg(target_os = "macos")]
    Dialect { names: &["xed"], line: LineArg::Flag, wait: WAIT },
    #[cfg(not(target_os = "macos"))]
    Dialect { names: &["xed"], line: LineArg::Plus, wait: WAIT },
    Dialect { names: &["kate"], line: LineArg::Flag, wait: BLOCK },
    Dialect {
        names: &[
            "idea",
            "pycharm",
            "webstorm",
            "goland",
            "clion",
            "phpstorm",
            "rubymine",
            "rider",
            "datagrip",
            "rustrover",
            "dataspell",
            "fleet",
        ],
        line: LineArg::Flag,
        wait: WAIT,
    },
];

/// A resolved editor invocation: the program to run, its full argument list, and whether it
/// wants the terminal.
///
/// A terminal editor paints in the pane and must be handed it outright. A graphical one opens
/// a window and never reads the terminal at all, so reviewr keeps it (`specs/input.md` Edit).
/// The wait flag is what tells them apart: an editor needs one exactly when it hands the file
/// to a window and returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorCommand {
    pub program: String,
    pub args: Vec<String>,
    pub wants_terminal: bool,
}

/// The command that opens `path` at `line`, or `None` when no editor is configured.
///
/// `configured` is the `editor` config key. Its value is the whole command, and `{file}` and
/// `{line}` substitute into it wherever they appear. A template that does not name `{file}` gets
/// the path appended, so a bare `editor = "hx"` still opens the file.
///
/// With no key, `visual` then `editor_env` supply the command, and its binary name selects a
/// dialect. An unrecognized binary opens the file without a line rather than guessing a flag it
/// may not accept. The binary name also decides who owns the pane, in every path
/// ([`wants_terminal`]).
pub fn resolve(
    configured: Option<&str>,
    visual: Option<&str>,
    editor_env: Option<&str>,
    path: &Path,
    line: u32,
) -> Option<EditorCommand> {
    // An absolute path can never be read as a flag, so no dialect needs a `--` guard.
    let file = path.to_string_lossy().into_owned();
    if let Some(template) = configured.filter(|t| !t.trim().is_empty()) {
        return Some(from_template(template, &file, line));
    }
    let value = visual
        .filter(|v| !v.trim().is_empty())
        .or_else(|| editor_env.filter(|v| !v.trim().is_empty()))?;
    let mut words = split_command(value).into_iter();
    let program = words.next()?;
    let mut args: Vec<String> = words.collect();
    let Some(dialect) = dialect_for(&program) else {
        args.push(file);
        let wants_terminal = wants_terminal(&program);
        return Some(EditorCommand { program, args, wants_terminal });
    };
    // The reviewer's own flags win: a `$EDITOR` that already waits never waits twice, because
    // not every editor's parser accepts a repeated flag. Either spelling counts as already set.
    if let Some(wait) = dialect.wait.filter(|w| !args.iter().any(|a| a == w.long || a == w.short)) {
        args.push(wait.long.to_owned());
    }
    match dialect.line {
        LineArg::Plus => {
            args.push(format!("+{line}"));
            args.push(file);
        }
        LineArg::Suffix => args.push(format!("{file}:{line}")),
        LineArg::Goto => {
            args.push("-g".to_owned());
            args.push(format!("{file}:{line}"));
        }
        LineArg::Flag => {
            args.push("--line".to_owned());
            args.push(line.to_string());
            args.push(file);
        }
    }
    let wants_terminal = wants_terminal(&program);
    Some(EditorCommand { program, args, wants_terminal })
}

/// Whether `program` draws in the pane it was launched from.
///
/// Its own name is the only signal, and it is the same signal in every path: an editor that
/// takes a wait flag has a window of its own, and everything else — a binary reviewr does not
/// know included — draws in the terminal. Unknown means the pane, which is the outcome a
/// terminal editor cannot survive being denied (`specs/input.md` Edit).
fn wants_terminal(program: &str) -> bool {
    dialect_for(program).is_none_or(|d| d.wait.is_none())
}

/// Split a command into words, honouring quotes.
///
/// A plain whitespace split cannot express `/Applications/Sublime Text.app/.../subl`, which is
/// how macOS spells most editor paths. Quoting is the only escape, since no shell runs the
/// command. A quote closes at its match or at the end of the string.
fn split_command(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    for ch in value.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => word.push(ch),
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
                started = true;
            }
            None if ch.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            None => {
                word.push(ch);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

/// The dialect for a binary, matched on its file name so an absolute `$EDITOR` resolves too.
fn dialect_for(program: &str) -> Option<&'static Dialect> {
    let name = Path::new(program).file_name()?.to_string_lossy().to_lowercase();
    DIALECTS.iter().find(|d| d.names.contains(&name.as_str()))
}

/// Substitute every `{file}` and `{line}` in `word`, in one pass.
///
/// One pass, so a substituted value is never itself searched: a path that spells `{line}` is a
/// file name, not a placeholder.
fn substitute(word: &str, file: &str, line: &str) -> String {
    let mut out = String::with_capacity(word.len());
    let mut rest = word;
    while let Some(at) = rest.find('{') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if let Some(tail) = rest.strip_prefix("{file}") {
            out.push_str(file);
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("{line}") {
            out.push_str(line);
            rest = tail;
        } else {
            out.push('{');
            rest = &rest['{'.len_utf8()..];
        }
    }
    out.push_str(rest);
    out
}

/// Build the command from a user template, substituting every `{file}` and `{line}`.
fn from_template(template: &str, file: &str, line: u32) -> EditorCommand {
    let named_file = template.contains("{file}");
    let line = line.to_string();
    let mut words = split_command(template).into_iter().map(|w| substitute(&w, file, &line));
    let program = words.next().unwrap_or_default();
    let mut args: Vec<String> = words.collect();
    if !named_file {
        args.push(file.to_owned());
    }
    let wants_terminal = wants_terminal(&program);
    EditorCommand { program, args, wants_terminal }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p() -> PathBuf {
        PathBuf::from("/repo/src/lib.rs")
    }

    fn env(value: &str) -> Option<EditorCommand> {
        resolve(None, None, Some(value), &p(), 41)
    }

    fn argv(cmd: &EditorCommand) -> String {
        format!("{} {}", cmd.program, cmd.args.join(" "))
    }

    #[test]
    fn terminal_editors_take_a_plus_line_and_never_wait() {
        for name in ["vi", "vim", "nvim", "nano", "micro", "kak", "emacs"] {
            assert_eq!(
                argv(&env(name).unwrap()),
                format!("{name} +41 /repo/src/lib.rs"),
                "{name} takes `+LINE` before the path and owns the pane already"
            );
        }
    }

    #[test]
    fn helix_takes_the_line_as_a_path_suffix() {
        assert_eq!(argv(&env("hx").unwrap()), "hx /repo/src/lib.rs:41");
        assert_eq!(argv(&env("helix").unwrap()), "helix /repo/src/lib.rs:41");
    }

    #[test]
    fn graphical_editors_wait_so_the_pane_does_not_repaint_first() {
        // The VS Code family, its forks included, takes `--goto path:line`.
        for name in ["code", "code-insiders", "codium", "cursor", "windsurf"] {
            assert_eq!(
                argv(&env(name).unwrap()),
                format!("{name} --wait -g /repo/src/lib.rs:41"),
                "{name} returns immediately without its wait flag"
            );
        }
        assert_eq!(argv(&env("zed").unwrap()), "zed --wait /repo/src/lib.rs:41");
        assert_eq!(argv(&env("subl").unwrap()), "subl --wait /repo/src/lib.rs:41");
        assert_eq!(argv(&env("bbedit").unwrap()), "bbedit --wait +41 /repo/src/lib.rs");
        assert_eq!(argv(&env("mate").unwrap()), "mate --wait --line 41 /repo/src/lib.rs");
        // `xed` is Xcode's opener on macOS and Mint's gedit fork elsewhere, and they disagree.
        #[cfg(target_os = "macos")]
        assert_eq!(argv(&env("xed").unwrap()), "xed --wait --line 41 /repo/src/lib.rs");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(argv(&env("xed").unwrap()), "xed --wait +41 /repo/src/lib.rs");
        // Kate blocks under its own flag name.
        assert_eq!(argv(&env("kate").unwrap()), "kate --block --line 41 /repo/src/lib.rs");
        // Every JetBrains launcher shares one CLI.
        for name in ["idea", "pycharm", "webstorm", "goland", "rider"] {
            assert_eq!(
                argv(&env(name).unwrap()),
                format!("{name} --wait --line 41 /repo/src/lib.rs")
            );
        }
    }

    #[test]
    fn a_wait_flag_the_reviewer_already_set_is_not_repeated() {
        // `EDITOR="code --wait"` is the documented git setup, so it arrives already waiting.
        assert_eq!(argv(&env("code --wait").unwrap()), "code --wait -g /repo/src/lib.rs:41");
        // The short spelling is the same request, and a second flag is what some parsers reject.
        assert_eq!(argv(&env("code -w").unwrap()), "code -w -g /repo/src/lib.rs:41");
        assert_eq!(argv(&env("subl -w").unwrap()), "subl -w /repo/src/lib.rs:41");
        assert_eq!(argv(&env("kate -b").unwrap()), "kate -b --line 41 /repo/src/lib.rs");
        assert_eq!(argv(&env("mvim -f").unwrap()), "mvim -f +41 /repo/src/lib.rs");
    }

    #[test]
    fn only_a_terminal_editor_is_handed_the_pane() {
        // The wait flag is the signal: an editor needs one exactly when it hands the file to a
        // window and returns, which is the same set that never reads the terminal.
        for name in ["vim", "nvim", "nano", "micro", "kak", "emacs", "hx", "helix"] {
            assert!(env(name).unwrap().wants_terminal, "{name} paints in the pane");
        }
        for name in ["code", "cursor", "zed", "subl", "idea", "kate", "mate", "mvim"] {
            assert!(!env(name).unwrap().wants_terminal, "{name} opens a window");
        }
        // A configured command spells its own arguments, but it is still one of these
        // binaries, and the same name answers the same question.
        let cfg = |t: &str| resolve(Some(t), None, None, &p(), 41).unwrap().wants_terminal;
        assert!(!cfg("code --wait -g {file}:{line}"), "the documented example keeps the pane");
        assert!(cfg("vim +{line} {file}"), "a terminal editor still takes it");
        // An unknown binary says nothing, and only one of the two guesses is survivable.
        assert!(env("myeditor").unwrap().wants_terminal);
        assert!(cfg("myed {file}"));
    }

    #[test]
    fn a_path_that_spells_a_placeholder_is_not_substituted_twice() {
        // `{line}.cshtml` is a real ASP.NET route file name. One pass, so the path lands whole.
        let path = PathBuf::from("/repo/routes/{line}.cshtml");
        let got = resolve(Some("code -g {file}:{line}"), None, None, &path, 41).unwrap();
        assert_eq!(argv(&got), "code -g /repo/routes/{line}.cshtml:41");
        // A brace that opens no placeholder is just a character.
        let plain = PathBuf::from("/repo/a.rs");
        let kept = resolve(Some("ed --at={line} {x} {file}"), None, None, &plain, 7).unwrap();
        assert_eq!(argv(&kept), "ed --at=7 {x} /repo/a.rs");
    }

    #[test]
    fn a_windowed_vim_is_told_to_stay_in_the_foreground() {
        // MacVim and gVim open a window and return, unlike every other vi-family binary.
        assert_eq!(argv(&env("mvim").unwrap()), "mvim -f +41 /repo/src/lib.rs");
        assert_eq!(argv(&env("gvim").unwrap()), "gvim -f +41 /repo/src/lib.rs");
        assert_eq!(argv(&env("vim").unwrap()), "vim +41 /repo/src/lib.rs", "terminal vim does not");
    }

    #[test]
    fn extra_flags_survive_and_an_absolute_binary_still_matches() {
        assert_eq!(
            argv(&env("nvim --clean").unwrap()),
            "nvim --clean +41 /repo/src/lib.rs",
            "the reviewer's own flags come before the ones the dialect adds"
        );
        assert_eq!(
            argv(&env("/opt/homebrew/bin/nvim").unwrap()),
            "/opt/homebrew/bin/nvim +41 /repo/src/lib.rs",
            "the dialect matches the file name, not the whole path"
        );
        assert_eq!(argv(&env("VIM").unwrap()), "VIM +41 /repo/src/lib.rs", "the match is caseless");
    }

    #[test]
    fn a_quoted_path_with_spaces_stays_one_word() {
        // The common macOS spelling. No shell runs the command, so quoting is the only escape.
        let subl = "/Applications/Sublime Text.app/Contents/SharedSupport/bin/subl";
        let cmd = env(&format!("\"{subl}\"")).unwrap();
        assert_eq!(cmd.program, subl, "the whole quoted path is the program");
        assert_eq!(
            cmd.args,
            ["--wait", "/repo/src/lib.rs:41"],
            "and the quoted path's own name still picks the dialect"
        );

        // Single quotes too, and a quoted argument after the program.
        let cmd = env(&format!("'{subl}' --project 'My Project.sublime-project'")).unwrap();
        assert_eq!(cmd.program, subl);
        assert_eq!(
            cmd.args,
            ["--project", "My Project.sublime-project", "--wait", "/repo/src/lib.rs:41"]
        );

        // The config template quotes the same way.
        let cmd =
            resolve(Some("'/opt/my editor' --at {line} {file}"), None, None, &p(), 41).unwrap();
        assert_eq!(cmd.program, "/opt/my editor");
        assert_eq!(cmd.args, ["--at", "41", "/repo/src/lib.rs"]);

        // An unterminated quote closes at the end rather than dropping the word.
        assert_eq!(env("\"/opt/my editor").unwrap().program, "/opt/my editor");
    }

    #[test]
    fn an_unknown_editor_opens_the_file_without_a_line() {
        assert_eq!(
            argv(&env("myeditor").unwrap()),
            "myeditor /repo/src/lib.rs",
            "guessing a flag an unknown editor may not accept would open a stray buffer"
        );
    }

    #[test]
    fn visual_outranks_editor_and_blank_values_fall_through() {
        assert_eq!(
            argv(&resolve(None, Some("hx"), Some("vim"), &p(), 41).unwrap()),
            "hx /repo/src/lib.rs:41"
        );
        assert_eq!(
            argv(&resolve(None, Some("  "), Some("vim"), &p(), 41).unwrap()),
            "vim +41 /repo/src/lib.rs"
        );
        assert_eq!(
            resolve(None, None, None, &p(), 41),
            None,
            "no editor anywhere resolves nothing"
        );
        assert_eq!(resolve(None, Some(""), Some(" "), &p(), 41), None);
    }

    #[test]
    fn the_config_template_owns_the_whole_command() {
        assert_eq!(
            argv(&resolve(Some("code -g {file}:{line}"), None, Some("vim"), &p(), 41).unwrap()),
            "code -g /repo/src/lib.rs:41",
            "the configured template outranks the environment and takes no added flags"
        );
        assert_eq!(
            argv(&resolve(Some("idea --line {line} --wait {file}"), None, None, &p(), 41).unwrap()),
            "idea --line 41 --wait /repo/src/lib.rs"
        );
        assert_eq!(
            argv(&resolve(Some("hx"), None, None, &p(), 41).unwrap()),
            "hx /repo/src/lib.rs",
            "a template naming no placeholder still gets the path"
        );
        assert_eq!(
            argv(&resolve(Some("myed {line} {file} {line}"), None, None, &p(), 41).unwrap()),
            "myed 41 /repo/src/lib.rs 41",
            "every occurrence substitutes"
        );
    }
}
