pub mod converse;
mod env;
mod ffi;
pub mod session;

use std::{cell::Cell, thread::JoinHandle};

use anyhow::Result;
use flume::{Receiver, Sender};
use thiserror::Error as ThisError;

use pam_sys::PamReturnCode;
use tracing::{info, warn};

#[derive(Debug, ThisError)]
pub enum PamError {
  #[error("{0}")]
  Error(String),
  #[error("{0}")]
  AuthError(String),
  #[error("abort error: {0}")]
  AbortError(String),
  #[error("conv error")]
  ConvError,
}

impl PamError {
  pub fn from_rc(prefix: &str, rc: PamReturnCode) -> PamError {
    match rc {
      PamReturnCode::ABORT => PamError::AbortError(format!("{}: {:?}", prefix, rc)),
      PamReturnCode::AUTH_ERR
      | PamReturnCode::MAXTRIES
      | PamReturnCode::CRED_EXPIRED
      | PamReturnCode::ACCT_EXPIRED
      | PamReturnCode::CRED_INSUFFICIENT
      | PamReturnCode::USER_UNKNOWN
      | PamReturnCode::PERM_DENIED
      | PamReturnCode::SERVICE_ERR => PamError::AuthError(format!("{}: {:?}", prefix, rc)),
      PamReturnCode::CONV_ERR => PamError::ConvError,
      _ => PamError::Error(format!("{}: {:?}", prefix, rc)),
    }
  }
}

pub enum PamMessage {
  Echo(String),
  Blind(String),
  Info(String),
  Error(String),
  Success,
}

struct ChannelConv {
  pw_rx: Receiver<String>,
  pam_tx: Sender<PamMessage>,
  cancel_rx: Receiver<()>,
  canceled: Cell<bool>,
}

impl ChannelConv {
  pub fn new(pw_rx: Receiver<String>, pam_tx: Sender<PamMessage>, cancel_rx: Receiver<()>) -> Self {
    ChannelConv {
      pw_rx,
      pam_tx,
      cancel_rx,
      canceled: Cell::new(false),
    }
  }
}

impl converse::Converse for ChannelConv {
  fn prompt_echo(&self, msg: &str) -> Result<String, ()> {
    if self.canceled.get() {
      return Err(());
    }

    self
      .pam_tx
      .send(PamMessage::Echo(msg.to_string()))
      .map_err(|_| ())?;

    flume::Selector::new()
      .recv(&self.pw_rx, |pw| {
        pw.map_err(|_| warn!("password channel dropped"))
      })
      .recv(&self.cancel_rx, |_| {
        self.canceled.set(true);
        Err(())
      })
      .wait()
  }

  fn prompt_blind(&self, msg: &str) -> Result<String, ()> {
    if self.canceled.get() {
      return Err(());
    }

    self
      .pam_tx
      .send(PamMessage::Blind(msg.to_string()))
      .map_err(|_| ())?;

    flume::Selector::new()
      .recv(&self.pw_rx, |pw| {
        pw.map_err(|_| warn!("password channel dropped"))
      })
      .recv(&self.cancel_rx, |_| {
        self.canceled.set(true);
        Err(())
      })
      .wait()
  }

  fn info(&self, msg: &str) -> Result<(), ()> {
    self
      .pam_tx
      .send(PamMessage::Info(msg.to_string()))
      .map_err(|err| warn!("send error: {err}"))
  }

  fn error(&self, msg: &str) -> Result<(), ()> {
    self
      .pam_tx
      .send(PamMessage::Error(msg.to_string()))
      .map_err(|err| warn!("send error: {err}"))
  }
}

pub struct PamThread {
  handle: JoinHandle<()>,
  cancel_tx: Sender<()>,
}

impl PamThread {
  pub fn start(app: &str, user: &str, pw_rx: Receiver<String>, pam_tx: Sender<PamMessage>) -> Self {
    info!("Starting PAM handler thread");
    let (cancel_tx, cancel_rx) = flume::unbounded::<()>();

    let app = app.to_string();
    let user = user.to_string();

    let handle = std::thread::spawn(move || 'session: loop {
      info!("Starting PAM session");
      let conv = ChannelConv::new(pw_rx.clone(), pam_tx.clone(), cancel_rx.clone());
      let conv = Box::pin(conv);
      let mut pam_session = session::PamSession::start(&app, &user, conv).unwrap();

      let err = match pam_session.authenticate(pam_sys::PamFlag::NONE) {
        Ok(()) => {
          pam_tx.send(PamMessage::Success).unwrap();
          pam_session.end().unwrap();
          break 'session;
        }
        Err(err) => err,
      };

      match err {
        PamError::Error(err) | PamError::AuthError(err) | PamError::AbortError(err) => {
          pam_tx.send(PamMessage::Error(err)).unwrap();
          pam_session.end().unwrap();
        }
        PamError::ConvError => {
          // This means the conversation was cancelled and the thread should exit
          break 'session;
        }
      }
    });

    PamThread { handle, cancel_tx }
  }

  pub fn cancel(self) {
    self.cancel_tx.send(()).unwrap();
    self.handle.join().unwrap();
  }

  pub fn end(self) {
    self.handle.join().unwrap();
  }
}
