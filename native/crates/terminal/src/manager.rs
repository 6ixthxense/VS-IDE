//! Multi-session manager (port of `TerminalService` + the IPC handlers).

use std::collections::HashMap;
use std::sync::Arc;

use crossbeam_channel::{unbounded, Receiver, Sender};
use parking_lot::Mutex;

use crate::error::TerminalError;
use crate::session::{
    ScrollCmd, Session, SessionConfig, TerminalEvent,
};
use crate::snapshot::GridSnapshot;

/// Owns all terminal sessions and the shared event channel.
pub struct TerminalManager {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    /// `set_working_directory` calls for sessions that do not exist yet (Electron's
    /// `pendingCwds`); applied as the working directory when the session is created.
    pending_cwds: Mutex<HashMap<String, String>>,
    tx: Sender<TerminalEvent>,
    rx: Receiver<TerminalEvent>,
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalManager {
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        TerminalManager {
            sessions: Mutex::new(HashMap::new()),
            pending_cwds: Mutex::new(HashMap::new()),
            tx,
            rx,
        }
    }

    /// Receiver for events of all sessions (cloneable; clones compete for events).
    pub fn events(&self) -> Receiver<TerminalEvent> {
        self.rx.clone()
    }

    /// Create a session. Like the Electron service this is idempotent: creating an id that
    /// already exists succeeds without doing anything.
    pub fn create(&self, id: &str, mut config: SessionConfig) -> Result<Arc<Session>, TerminalError> {
        let mut map = self.sessions.lock();
        if let Some(existing) = map.get(id) {
            return Ok(existing.clone());
        }
        if let Some(pending) = self.pending_cwds.lock().remove(id) {
            let p = std::path::PathBuf::from(pending);
            if p.is_dir() {
                config.cwd = Some(p);
            }
        }
        let session = Session::spawn(id, config, self.tx.clone())?;
        map.insert(id.to_string(), session.clone());
        Ok(session)
    }

    pub fn get(&self, id: &str) -> Option<Arc<Session>> {
        self.sessions.lock().get(id).cloned()
    }

    fn require(&self, id: &str) -> Result<Arc<Session>, TerminalError> {
        self.get(id).ok_or_else(|| TerminalError::NotFound(id.to_string()))
    }

    pub fn ids(&self) -> Vec<String> {
        let mut v: Vec<String> = self.sessions.lock().keys().cloned().collect();
        v.sort();
        v
    }

    pub fn write(&self, id: &str, data: &[u8]) -> Result<(), TerminalError> {
        self.require(id)?.write(data)
    }

    pub fn paste(&self, id: &str, text: &str) -> Result<(), TerminalError> {
        self.require(id)?.paste(text)
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), TerminalError> {
        self.require(id)?.resize(cols, rows)
    }

    /// `cd` an existing session; if the session does not exist yet the path is remembered and
    /// used as its working directory when it is created.
    pub fn set_working_directory(&self, id: &str, path: &str) -> Result<(), TerminalError> {
        match self.get(id) {
            Some(s) => s.change_directory(path),
            None => {
                self.pending_cwds.lock().insert(id.to_string(), path.to_string());
                Ok(())
            }
        }
    }

    pub fn snapshot(&self, id: &str) -> Result<GridSnapshot, TerminalError> {
        Ok(self.require(id)?.snapshot())
    }

    pub fn scroll(&self, id: &str, cmd: ScrollCmd) -> Result<(), TerminalError> {
        self.require(id)?.scroll(cmd);
        Ok(())
    }

    /// Kill the process but keep the session (and its final screen) available.
    pub fn kill(&self, id: &str) -> Result<(), TerminalError> {
        self.require(id)?.kill()
    }

    /// Kill the process and forget the session. Unknown ids are ignored.
    pub fn close(&self, id: &str) {
        self.pending_cwds.lock().remove(id);
        let removed = self.sessions.lock().remove(id);
        if let Some(s) = removed {
            s.close();
        }
    }

    /// Close every session (call on app exit).
    pub fn close_all(&self) {
        let all: Vec<Arc<Session>> = self.sessions.lock().drain().map(|(_, s)| s).collect();
        for s in all {
            s.close();
        }
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        self.close_all();
    }
}
