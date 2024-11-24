pub mod converse;
mod env;
mod ffi;
pub mod session;

use flume::{Receiver, Sender};
use thiserror::Error as ThisError;

use pam_sys::PamReturnCode;

#[derive(Debug, ThisError)]
pub enum PamError {
  #[error("{0}")]
  Error(String),
  #[error("{0}")]
  AuthError(String),
  #[error("abort error: {0}")]
  AbortError(String),
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
      _ => PamError::Error(format!("{}: {:?}", prefix, rc)),
    }
  }
}

pub enum PamRequest {
  Echo(String),
  Blind(String),
  Info(String),
  Error(String),
}

pub struct ChannelConv {
  rx: Receiver<String>,
  tx: Sender<PamRequest>,
}

impl ChannelConv {
  pub fn new(rx: Receiver<String>, tx: Sender<PamRequest>) -> Self {
    ChannelConv { rx, tx }
  }
}

impl converse::Converse for ChannelConv {
  fn prompt_echo(&self, msg: &str) -> Result<String, ()> {
    self.tx.send(PamRequest::Echo(msg.to_string())).unwrap();
    self.rx.recv().map_err(|_| ())
  }

  fn prompt_blind(&self, msg: &str) -> Result<String, ()> {
    self.tx.send(PamRequest::Blind(msg.to_string())).unwrap();
    self.rx.recv().map_err(|_| ())
  }

  fn info(&self, msg: &str) -> Result<(), ()> {
    self.tx.send(PamRequest::Info(msg.to_string())).unwrap();
    Ok(())
  }

  fn error(&self, msg: &str) -> Result<(), ()> {
    self.tx.send(PamRequest::Error(msg.to_string())).unwrap();
    Ok(())
  }
}
