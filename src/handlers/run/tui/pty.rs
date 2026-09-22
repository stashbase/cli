use crate::handlers::run::tui::TuiCommand;
use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use portable_pty::{native_pty_system, Child, ChildKiller, ExitStatus, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::{mpsc::Sender, Arc, Mutex};
use std::thread::JoinHandle;

static ACTIVE_CHILD: Lazy<Mutex<Option<Box<dyn ChildKiller + Send + Sync>>>> =
    Lazy::new(|| Mutex::new(None));

#[derive(Debug)]
pub enum TuiEvent {
    Output(Vec<u8>),
    OutputClosed,
}

pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn Child + Send + Sync>,
    _reader: JoinHandle<()>,
}

impl PtySession {
    pub fn spawn(command: TuiCommand, size: PtySize, events: Sender<TuiEvent>) -> Result<Self> {
        let pair = native_pty_system()
            .openpty(size)
            .context("failed to open a pseudo-terminal")?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .context("failed to open the pseudo-terminal reader")?;
        let writer = Arc::new(Mutex::new(
            pair.master
                .take_writer()
                .context("failed to open the pseudo-terminal writer")?,
        ));
        let child = pair
            .slave
            .spawn_command(command.into_builder())
            .context("failed to start agent under --tui")?;
        drop(pair.slave);

        *ACTIVE_CHILD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(child.clone_killer());
        let reader = std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        if events
                            .send(TuiEvent::Output(buffer[..count].to_vec()))
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            let _ = events.send(TuiEvent::OutputClosed);
        });

        Ok(Self {
            master: pair.master,
            writer,
            child,
            _reader: reader,
        })
    }

    #[cfg(test)]
    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        self.writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .write_all(bytes)
            .context("failed to write to the agent pseudo-terminal")
    }

    pub fn input_writer(&self) -> Arc<Mutex<Box<dyn Write + Send>>> {
        self.writer.clone()
    }

    pub fn resize(&self, size: PtySize) -> Result<()> {
        self.master
            .resize(size)
            .context("failed to resize the agent pseudo-terminal")
    }

    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        self.child
            .try_wait()
            .context("failed to query the agent process")
    }

    pub fn interrupt(&mut self) -> Result<()> {
        self.child
            .kill()
            .context("failed to stop the agent process")
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        ACTIVE_CHILD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
    }
}

pub fn interrupt_active_child() {
    if let Some(child) = ACTIVE_CHILD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_mut()
    {
        let _ = child.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::{PtySession, TuiEvent};
    use crate::handlers::run::tui::TuiCommand;
    use portable_pty::PtySize;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[cfg(unix)]
    #[test]
    fn pty_session_streams_input_output_resize_and_exit_status() {
        let (events, receiver) = mpsc::channel();
        let command = TuiCommand {
            program: "sh".into(),
            args: vec![
                "-c".into(),
                "IFS= read -r line; stty size; printf 'got:%s\\n' \"$line\"; exit 17".into(),
            ],
            cwd: std::env::current_dir().unwrap(),
            env: Vec::new(),
            env_removals: Vec::new(),
        };
        let mut session = PtySession::spawn(command, size(4, 20), events).unwrap();
        session.resize(size(6, 30)).unwrap();
        session.write(b"hello\r").unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        let status = loop {
            while let Ok(event) = receiver.try_recv() {
                if let TuiEvent::Output(bytes) = event {
                    output.extend(bytes);
                }
            }
            if let Some(status) = session.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "PTY child did not exit");
            std::thread::sleep(Duration::from_millis(10));
        };
        while let Ok(event) = receiver.recv_timeout(Duration::from_millis(50)) {
            if let TuiEvent::Output(bytes) = event {
                output.extend(bytes);
            }
        }

        assert!(output.windows(5).any(|bytes| bytes == b"hello"));
        assert!(output.windows(4).any(|bytes| bytes == b"6 30"));
        assert_eq!(status.exit_code(), 17);
    }

    fn size(rows: u16, cols: u16) -> PtySize {
        PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}
