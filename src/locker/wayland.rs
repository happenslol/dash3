use std::{sync::{Arc, Mutex}, time::Duration};

use anyhow::Result;
use gdk4_wayland::prelude::WaylandSurfaceExtManual;
use gtk4::{glib::translate::ToGlibPtr, prelude::*};
use smithay_client_toolkit::{
  output::{OutputHandler, OutputState},
  reexports::{
    calloop::{channel::channel, EventLoop, LoopHandle},
    calloop_wayland_source::WaylandSource,
  },
  registry::{ProvidesRegistryState, RegistryState},
  registry_handlers,
  session_lock::{
    SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface,
    SessionLockSurfaceConfigure,
  },
};
use tracing::{error, info};
use wayland_backend::client::Backend;
use wayland_client::{
  globals::registry_queue_init,
  protocol::{
    wl_buffer,
    wl_output::{self, WlOutput},
    wl_surface::WlSurface,
  },
  Connection, QueueHandle,
};

use crate::{create_window, pam::PamMessage, SendApp};

struct WaylandState {
  app: SendApp,
  running: bool,
  loop_handle: LoopHandle<'static, Self>,
  conn: Connection,
  session_lock_state: SessionLockState,
  session_lock: Option<SessionLock>,
  registry_state: RegistryState,
  output_state: OutputState,
  surfaces: Arc<Mutex<Vec<SessionLockSurface>>>,

  // app state
  is_loading: futures_signals::signal::Mutable<bool>,
  pw_tx: flume::Sender<String>,
  pam_rx: flume::Receiver<PamMessage>,
}

impl WaylandState {
  fn unlock(&mut self) {
    let Some(session_lock) = self.session_lock.take() else {
      error!("session lock not initialized");
      return;
    };

    session_lock.unlock();

    // Sync connection to make sure compostor receives destroy
    if let Err(err) = self.conn.roundtrip() {
      error!("failed to roundtrip after unlocking session: {err}");
    };

    // Then we can exit
    self.running = false;
  }

  fn create_lock_surface(&mut self, qh: &QueueHandle<Self>, output: &WlOutput) -> Result<()> {
    let is_loading = self.is_loading.clone();
    let pw_tx = self.pw_tx.clone();
    let pam_rx = self.pam_rx.clone();

    let session_lock = self.session_lock.as_ref().unwrap().clone();
    let app = self.app.clone();
    let qh = qh.clone();
    let output = output.clone();
    let surfaces = self.surfaces.clone();

    gtk4::glib::idle_add(move || {
      info!("running on main thread");

      let mut surfaces = surfaces.lock().unwrap();
      let app = app.clone();
      let win = create_window(&app.0, is_loading.clone(), pw_tx.clone(), pam_rx.clone());
      WidgetExt::realize(&win);

      let surface = win.surface().unwrap();
      let surface = surface.downcast::<gdk4_wayland::WaylandSurface>().unwrap();
      let wl_surface: WlSurface = surface.wl_surface().unwrap();

      info!("creating");
      let surface = session_lock.create_lock_surface(wl_surface, &output, &qh);
      info!("pushing");
      surfaces.push(surface);
      info!("presenting window");
      win.present();

      gtk4::glib::ControlFlow::Break
    });

    Ok(())
  }
}

pub fn lock_session(
  app: SendApp,
  pw_tx: flume::Sender<String>,
  pam_rx: flume::Receiver<PamMessage>,
  is_loading: futures_signals::signal::Mutable<bool>,
) -> Result<()> {
  let (_unlock_tx, unlock_rx) = channel::<()>();

  let display = gtk4::gdk::Display::default().unwrap();
  let wl_display = display.downcast::<gdk4_wayland::WaylandDisplay>().unwrap();

  let wl_display =
    unsafe { gdk4_wayland::ffi::gdk_wayland_display_get_wl_display(wl_display.to_glib_none().0) };

  let wl_backend = unsafe { Backend::from_foreign_display(wl_display as *mut _) };
  let wl_conn = Connection::from_backend(wl_backend);

  let (globals, event_queue) = registry_queue_init(&wl_conn)?;

  let qh: QueueHandle<WaylandState> = event_queue.handle();

  let _thread_handle = std::thread::spawn(move || {
    let mut event_loop: EventLoop<WaylandState> = match EventLoop::try_new() {
      Ok(event_loop) => event_loop,
      Err(err) => {
        error!("Failed to create event loop: {err}");
        // exit
        return;
      }
    };

    let loop_handle = event_loop.handle();

    // if let Err(err) = loop_handle.insert_source(unlock_rx, |_, _, app_data| app_data.unlock()) {
    //   error!("failed to insert unlock source: {err}");
    //   // exit
    //   return;
    // }

    let mut wl_state = WaylandState {
      app,
      running: true,
      output_state: OutputState::new(&globals, &qh),
      registry_state: RegistryState::new(&globals),
      loop_handle,
      conn: wl_conn.clone(),
      session_lock_state: SessionLockState::new(&globals, &qh),
      session_lock: None,
      is_loading,
      pw_tx,
      pam_rx,
      surfaces: Arc::new(Mutex::new(Vec::new())),
    };

    let session_lock = match wl_state.session_lock_state.lock(&qh) {
      Ok(session_lock) => session_lock,
      Err(err) => {
        error!("Compositor does not support ext_session_lock_v1: {err}");
        // exit
        return;
      }
    };

    if let Err(err) = WaylandSource::new(wl_conn.clone(), event_queue).insert(event_loop.handle()) {
      error!("failed to insert wayland source: {err}");
      // exit
      return;
    }

    wl_state.session_lock = Some(session_lock);
    for output in wl_state.output_state.outputs() {
      info!("creating lock surface");
      wl_state
        .create_lock_surface(&qh, &output)
        .unwrap_or_else(|err| {
          error!("failed to create lock surface: {err}");
        });
    }

    while wl_state.running {
      event_loop
        .dispatch(Duration::from_millis(16), &mut wl_state)
        .unwrap_or_else(|err| {
          error!("failed to dispatch event loop: {err}");
        });
    }

    // exit 0
  });

  Ok(())
}

impl ProvidesRegistryState for WaylandState {
  fn registry(&mut self) -> &mut RegistryState {
    &mut self.registry_state
  }
  registry_handlers![OutputState,];
}

impl OutputHandler for WaylandState {
  fn output_state(&mut self) -> &mut OutputState {
    &mut self.output_state
  }

  fn new_output(
    &mut self,
    _conn: &Connection,
    _qh: &QueueHandle<Self>,
    _output: wl_output::WlOutput,
  ) {
  }

  fn update_output(
    &mut self,
    _conn: &Connection,
    _qh: &QueueHandle<Self>,
    _output: wl_output::WlOutput,
  ) {
  }

  fn output_destroyed(
    &mut self,
    _conn: &Connection,
    _qh: &QueueHandle<Self>,
    _output: wl_output::WlOutput,
  ) {
  }
}

impl SessionLockHandler for WaylandState {
  fn locked(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _session_lock: SessionLock) {}

  fn finished(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _session_lock: SessionLock) {
    self.running = false;
  }

  fn configure(
    &mut self,
    _conn: &Connection,
    _qh: &QueueHandle<Self>,
    _session_lock_surface: SessionLockSurface,
    _configure: SessionLockSurfaceConfigure,
    _serial: u32,
  ) {
  }
}

smithay_client_toolkit::delegate_output!(WaylandState);
smithay_client_toolkit::delegate_session_lock!(WaylandState);
smithay_client_toolkit::delegate_registry!(WaylandState);
wayland_client::delegate_noop!(WaylandState: ignore wl_buffer::WlBuffer);
