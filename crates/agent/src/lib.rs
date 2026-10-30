//! charted's in-app agent. The desktop's own agent (Claude Code, Codex) builds report
//! pages and edits the model through the tools here, served to it over a control
//! socket in the running app by `charted mcp`; every change is an ordinary commit, so
//! it shows live and undoes like the user's own (one undo step per request).

pub mod builders;
pub mod catalog;
pub mod layout;
pub mod names;
pub mod tools;
