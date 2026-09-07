use stageswap_core::{Command, RestartTarget};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandDispatch {
    Queued,
    Coalesced,
    Busy,
    Closed,
}

impl CommandDispatch {
    pub const fn is_accepted(self) -> bool {
        matches!(self, Self::Queued | Self::Coalesced)
    }
}

#[derive(Default)]
struct CoalescedCommands {
    settings: Option<Command>,
    output_mode: Option<Command>,
}

impl CoalescedCommands {
    fn replace(&mut self, command: Command) -> bool {
        match command {
            command @ (Command::UpdateSettings(_) | Command::ReloadSettings(_)) => {
                // Settings include the output mode and supersede an older mode command.
                self.output_mode = None;
                self.settings.replace(command).is_some()
            }
            command @ Command::SetMode(_) => self.output_mode.replace(command).is_some(),
            _ => unreachable!("only coalescible commands enter the coalesced mailbox"),
        }
    }

    fn take(&mut self) -> Option<Command> {
        self.settings.take().or_else(|| self.output_mode.take())
    }
}

pub(crate) struct CommandMailbox {
    ordered: SyncSender<Command>,
    coalesced: Arc<Mutex<CoalescedCommands>>,
    shutdown: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    pending_restarts: Arc<Mutex<HashSet<RestartTarget>>>,
}

pub(crate) struct CommandInbox {
    ordered: Receiver<Command>,
    coalesced: Arc<Mutex<CoalescedCommands>>,
    shutdown: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    pending_restarts: Arc<Mutex<HashSet<RestartTarget>>>,
}

impl CommandMailbox {
    pub(crate) fn bounded(capacity: usize) -> (Self, CommandInbox) {
        let (ordered, receiver) = mpsc::sync_channel(capacity);
        let coalesced = Arc::new(Mutex::new(CoalescedCommands::default()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let closed = Arc::new(AtomicBool::new(false));
        let pending_restarts = Arc::new(Mutex::new(HashSet::new()));
        (
            Self {
                ordered,
                coalesced: Arc::clone(&coalesced),
                shutdown: Arc::clone(&shutdown),
                closed: Arc::clone(&closed),
                pending_restarts: Arc::clone(&pending_restarts),
            },
            CommandInbox {
                ordered: receiver,
                coalesced,
                shutdown,
                closed,
                pending_restarts,
            },
        )
    }

    pub(crate) fn dispatch(&self, command: Command) -> CommandDispatch {
        if self.closed.load(Ordering::Acquire) {
            return CommandDispatch::Closed;
        }
        if matches!(command, Command::Exit) {
            self.request_shutdown();
            return CommandDispatch::Queued;
        }
        if matches!(
            command,
            Command::SetMode(_) | Command::UpdateSettings(_) | Command::ReloadSettings(_)
        ) {
            let replaced = self
                .coalesced
                .lock()
                .expect("coalesced command lock is not poisoned")
                .replace(command);
            return if replaced {
                CommandDispatch::Coalesced
            } else {
                CommandDispatch::Queued
            };
        }
        let restart = match &command {
            Command::Restart(target) => Some(*target),
            _ => None,
        };
        if let Some(target) = restart
            && !self
                .pending_restarts
                .lock()
                .expect("pending restart lock is not poisoned")
                .insert(target)
        {
            return CommandDispatch::Coalesced;
        }
        match self.ordered.try_send(command) {
            Ok(()) => CommandDispatch::Queued,
            Err(mpsc::TrySendError::Full(_)) => {
                self.remove_pending_restart(restart);
                CommandDispatch::Busy
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.remove_pending_restart(restart);
                CommandDispatch::Closed
            }
        }
    }

    pub(crate) fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
    }

    fn remove_pending_restart(&self, restart: Option<RestartTarget>) {
        if let Some(target) = restart {
            self.pending_restarts
                .lock()
                .expect("pending restart lock is not poisoned")
                .remove(&target);
        }
    }
}

impl CommandInbox {
    pub(crate) fn shutdown_requested(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
    }

    pub(crate) fn recv_timeout(&self, timeout: Duration) -> Option<Command> {
        match self.ordered.recv_timeout(timeout) {
            Ok(command) => Some(self.mark_dequeued(command)),
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {
                self.take_coalesced()
            }
        }
    }

    pub(crate) fn try_recv(&self) -> Option<Command> {
        self.ordered
            .try_recv()
            .ok()
            .map(|command| self.mark_dequeued(command))
            .or_else(|| self.take_coalesced())
    }

    fn take_coalesced(&self) -> Option<Command> {
        self.coalesced
            .lock()
            .expect("coalesced command lock is not poisoned")
            .take()
    }

    fn mark_dequeued(&self, command: Command) -> Command {
        if let Command::Restart(target) = &command {
            self.pending_restarts
                .lock()
                .expect("pending restart lock is not poisoned")
                .remove(target);
        }
        command
    }
}

impl Drop for CommandInbox {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stageswap_core::{AppConfig, OutputMode, RestartTarget};

    #[test]
    fn flow_ordered_queue_reports_busy_without_blocking() {
        let (mailbox, inbox) = CommandMailbox::bounded(1);
        assert_eq!(mailbox.dispatch(Command::Start), CommandDispatch::Queued);
        assert_eq!(
            mailbox.dispatch(Command::Restart(RestartTarget::Webcam)),
            CommandDispatch::Busy
        );
        assert_eq!(inbox.try_recv(), Some(Command::Start));
    }

    #[test]
    fn flow_settings_and_mode_are_last_wins() {
        let (mailbox, inbox) = CommandMailbox::bounded(1);
        let first = AppConfig {
            cursor_visible: false,
            ..AppConfig::default()
        };
        let mut latest = first.clone();
        latest.cursor_visible = true;
        assert_eq!(
            mailbox.dispatch(Command::UpdateSettings(Box::new(first))),
            CommandDispatch::Queued
        );
        assert_eq!(
            mailbox.dispatch(Command::UpdateSettings(Box::new(latest.clone()))),
            CommandDispatch::Coalesced
        );
        assert_eq!(
            mailbox.dispatch(Command::SetMode(OutputMode::ForceCamera)),
            CommandDispatch::Queued
        );
        assert_eq!(
            mailbox.dispatch(Command::SetMode(OutputMode::ForceScreen)),
            CommandDispatch::Coalesced
        );
        assert_eq!(
            inbox.try_recv(),
            Some(Command::UpdateSettings(Box::new(latest)))
        );
        assert_eq!(
            inbox.try_recv(),
            Some(Command::SetMode(OutputMode::ForceScreen))
        );
    }

    #[test]
    fn flow_settings_and_modes_preserve_the_latest_output_intent() {
        let (mailbox, inbox) = CommandMailbox::bounded(1);
        mailbox.dispatch(Command::SetMode(OutputMode::ForceScreen));
        mailbox.dispatch(Command::ReloadSettings(Box::default()));
        assert!(matches!(inbox.try_recv(), Some(Command::ReloadSettings(_))));
        assert_eq!(inbox.try_recv(), None);

        mailbox.dispatch(Command::UpdateSettings(Box::default()));
        mailbox.dispatch(Command::SetMode(OutputMode::ForceCamera));
        assert!(matches!(inbox.try_recv(), Some(Command::UpdateSettings(_))));
        assert_eq!(
            inbox.try_recv(),
            Some(Command::SetMode(OutputMode::ForceCamera))
        );
    }

    #[test]
    fn flow_shutdown_bypasses_a_full_queue() {
        let (mailbox, inbox) = CommandMailbox::bounded(1);
        assert_eq!(mailbox.dispatch(Command::Start), CommandDispatch::Queued);
        assert_eq!(mailbox.dispatch(Command::Exit), CommandDispatch::Queued);
        assert!(inbox.shutdown_requested());
    }

    #[test]
    fn flow_duplicate_pending_restart_is_coalesced_but_a_later_retry_is_allowed() {
        let (mailbox, inbox) = CommandMailbox::bounded(2);
        let restart = Command::Restart(RestartTarget::ScreenCapture);
        assert_eq!(mailbox.dispatch(restart.clone()), CommandDispatch::Queued);
        assert_eq!(
            mailbox.dispatch(restart.clone()),
            CommandDispatch::Coalesced
        );
        assert_eq!(inbox.try_recv(), Some(restart.clone()));
        assert_eq!(mailbox.dispatch(restart.clone()), CommandDispatch::Queued);
        assert_eq!(inbox.try_recv(), Some(restart));
    }
}
