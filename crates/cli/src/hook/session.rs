//! The Stop hook's memory between runs.
//!
//! Each round is a separate process, so the only thing that carries a count
//! from one stop to the next is a file. It lives beside the index rather than
//! in the repository: a counter in the tree is a file the agent can read, edit
//! and commit, and this one exists to keep the agent honest.

use std::path::{Path, PathBuf};

use anyhow::Context;
use locrin_core::index::cache_path;
use serde::Deserialize;
use serde_json::json;

/// How many times one session may be sent back before the hook stands down.
///
/// Three is spec 5.2's cap. It sits below Claude Code's own limit of eight
/// consecutive blocks so the hook decides when to give up, and the person is
/// told about it, rather than the loop ending in the agent framework with no
/// explanation.
pub const MAX_ROUNDS: u32 = 3;

/// The Stop hook's memory of one Claude Code session: how many times it has
/// already sent the agent back.
pub struct Session {
    path: PathBuf,
    pub rounds: u32,
}

/// The file's shape. It is one number today and a struct anyway, so a later
/// field can be added without every existing session file becoming unreadable.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Stored {
    rounds: u32,
}

/// The file name for a session id.
///
/// A session id is data from a payload this process did not write, and it ends
/// up in a path, so it may not contain anything that navigates: an id of
/// `../../../x` must not choose where the file goes. Claude Code's ids are
/// UUIDs and survive this untouched; anything else has its unsafe bytes spelled
/// out, which keeps distinct ids in distinct files.
fn file_stem(session_id: &str) -> String {
    if session_id.is_empty() {
        return "anonymous".to_string();
    }
    let mut out = String::with_capacity(session_id.len());
    for b in session_id.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    // A name no file system would take is worse than two absurd ids sharing a
    // counter, which costs them nothing but a shared round budget.
    out.truncate(100);
    out
}

impl Session {
    /// The session's counter as the last run left it.
    ///
    /// A file that is missing, unreadable or not what this hook wrote is round
    /// zero. None of those is worth stalling a stop over: the file is
    /// disposable state, and reading it wrong at worst spends the agent's three
    /// rounds again.
    pub fn load(root: &Path, session_id: &str) -> Session {
        let path = cache_path(root)
            .parent()
            .expect("cache_path always names a file inside a directory")
            .join("sessions")
            .join(format!("{}.json", file_stem(session_id)));
        let rounds = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Stored>(&text).ok())
            .map(|stored| stored.rounds)
            .unwrap_or(0);
        Session { path, rounds }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let dir = self.path.parent().expect("load always builds a path with a parent");
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        std::fs::write(&self.path, json!({ "rounds": self.rounds }).to_string())
            .with_context(|| format!("writing {}", self.path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The counter has to outlive the process, because every round is a separate
    /// run of the hook: what one run saved is the only thing the next one knows
    /// about the session it is answering for.
    #[test]
    fn session_round_trips_and_is_zero_when_absent() {
        let _env = crate::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let repo = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        std::env::set_var("LOCRIN_CACHE_DIR", cache.path());
        let root = repo.path();

        assert_eq!(Session::load(root, "s1").rounds, 0, "a session the hook has never seen is at round zero");

        let mut s = Session::load(root, "s1");
        s.rounds = 2;
        s.save().unwrap();
        assert_eq!(Session::load(root, "s1").rounds, 2);
        // Two sessions in one repository count separately: the cap is per
        // session, not per repository.
        assert_eq!(Session::load(root, "s2").rounds, 0);

        // The file lives beside the index, which is outside the repository. A
        // counter written into the tree would be a file the agent could see, edit
        // and commit.
        assert!(!s.path.starts_with(root), "{}", s.path.display());
        assert!(s.path.starts_with(cache.path()), "{}", s.path.display());

        // Garbage where the counter should be reads as round zero rather than
        // stalling a stop over a file this hook wrote itself.
        std::fs::write(&s.path, "not json at all").unwrap();
        assert_eq!(Session::load(root, "s1").rounds, 0);

        // A session id is data from a payload, and one shaped like a path must
        // not decide where the file goes.
        let stray = Session::load(root, "../../escape");
        stray.save().unwrap();
        assert_eq!(stray.path.parent(), s.path.parent(), "{}", stray.path.display());

        // A payload with no session id still gets somewhere to count.
        assert!(Session::load(root, "").path.ends_with("anonymous.json"));

        std::env::remove_var("LOCRIN_CACHE_DIR");
    }
}
