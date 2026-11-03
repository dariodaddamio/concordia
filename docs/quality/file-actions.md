# File actions and independent editor

Workspace/input W3/W4 share `file_open::resolve_action`. File links, browser selections, search
results and explicit editor commands use the same argument-array planning and identity checks.
The actions are **Preview here**, **Split off**, and **Open in editor**. Preview remains the
default. Explicit `/open` and `/preview` retain their internal-pane behavior.

Press F4 on a selected file-browser row or focused file pane to choose an action. Alt-clicking
a transcript file link opens the same menu. Its Always choices persist an opening preference
in Concordia-owned settings; they do not rewrite config.toml. The optional `[ui].file_action`
default accepts `preview`, `split` or `editor`; an explicit stored preference wins. Closing the
menu preserves the draft. Search retains its exact verified snapshot for an internal preview;
failed actions retain the search screen and display the error.

Split off starts `concordia file-edit FILE` through the existing WezTerm host adapter, carrying
the canonical path and one-based line/column as arguments. Missing host support is visible.
The new process draws the same file panes at full width and reuses their editing, undo, syntax,
formatted/raw modes, save conflicts and recovery. It does not enter the chat dispatcher, probe
providers or start a model turn. Ctrl+Q requests close; dirty files use the existing explicit
save/discard/cancel flow. Opening a separate editor uses the disk version and retains parent
buffers. Launcher success does not establish that an edit or save completed.

Each independent editor creates its own held recovery lease under the separate file-editors
state directory. `file-edit FILE --list-recovery` lists unlocked records for the exact current
workspace. `file-edit FILE --recover ID` claims only that explicitly selected record, verifies
that it contains the exact file, and retains unsaved bytes without overwriting disk. A live
editor's lease cannot be claimed by another process. Recovery never uses the latest/focused
pane, another conversation's lease, or an automatic model continuation.

`[editor].launch` is `terminal` by default; existing terminal configuration keeps its meaning.
Explicit `gui` launches a configured native executable directly, without requiring WezTerm:

```toml
[editor]
launch = "gui"
program = "C:/Editors/Code.exe"
args = ["--goto", "{file}:{line}:{column}"]
```

Program resolution refuses batch/shell shims; select a native executable rather than a command
string or association. GUI mode requires an explicit program. Template values expand within
individual arguments and never undergo shell parsing. Ordinary GUI/standalone terminal windows
remain independent after startup. Pane-host launcher processes have bounded time/stderr and
tracked cleanup on failure; closing a menu cannot kill a successfully detached user's editor.
Missing/retargeted/linked/sensitive files are refused at action resolution. Native provider tools,
authentication, dirty buffers and global host configuration are unchanged.
Canonical Windows local-drive references accept the native `\\?\` prefix; UNC/device namespaces
remain refused. The independent editor uses the same VT input reader as chat, preserving CRLF
paste bytes, and flushes its owned recovery record on terminal-input failure.

Existing argument/config checks cover GUI position arguments, preserved placeholder/path data,
missing hosts and split arguments. The existing file-editor terminal journey is extended to
exercise independent save, crash recovery and dirty quit with zero provider prompts. Separate
physical-host and native-editor validation remains required for a release-qualified claim.
