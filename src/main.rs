use std::path::PathBuf;

use futures_signals::signal::SignalExt;
use gtk4::{
  gdk::Display,
  glib::{self},
  prelude::*,
  style_context_add_provider_for_display, Application, ApplicationWindow, CssProvider,
  STYLE_PROVIDER_PRIORITY_APPLICATION,
};
use notify::Watcher;
use pam::PamMessage;
use tracing::info;

mod locker;
mod pam;
mod scrambler;

fn load_css() -> String {
  grass::from_path("./src/styles.scss", &grass::Options::default()).unwrap()
}

#[derive(Clone)]
struct SendApp(pub gtk4::Application);

unsafe impl Send for SendApp {}

fn main() -> glib::ExitCode {
  tracing_subscriber::fmt::init();
  gtk4::gio::resources_register_include!("dash3.gresource").unwrap();

  let app = Application::builder()
    .application_id("lol.happens.dash3")
    .build();

  let (style_reload_tx, style_reload_rx) = flume::unbounded::<()>();
  app.connect_startup(move |_| {
    let display = Display::default().unwrap();

    let style_provider = CssProvider::new();
    style_provider.load_from_string(&load_css());

    style_context_add_provider_for_display(
      &display,
      &style_provider,
      STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let move_style_provider = style_provider.clone();
    let move_style_reload_rx = style_reload_rx.clone();
    glib::spawn_future_local(async move {
      while let Ok(()) = move_style_reload_rx.recv_async().await {
        move_style_provider.load_from_string(&load_css());
      }
    });
  });

  let mut watcher = notify::INotifyWatcher::new(
    move |ev: Result<notify::Event, notify::Error>| match ev {
      Ok(ev) => {
        if !ev.kind.is_modify() {
          return;
        }

        style_reload_tx.send(()).unwrap();
      }
      Err(e) => println!("Error: {:?}", e),
    },
    notify::Config::default(),
  )
  .unwrap();

  watcher
    .watch(
      &PathBuf::from("./src/styles.scss"),
      notify::RecursiveMode::NonRecursive,
    )
    .unwrap();

  let (pam_tx, pam_rx) = flume::unbounded::<PamMessage>();
  let (pw_tx, pw_rx) = flume::unbounded::<String>();
  let handle = pam::PamThread::start("dash3", "happens", pw_rx, pam_tx);

  // Keep the app open even if there are no windows
  let _hold = app.hold();

  app.connect_activate(move |app| activate(app, pam_rx.clone(), pw_tx.clone()));
  let exit_code = app.run();
  handle.end();
  exit_code
}

fn activate(app: &Application, pam_rx: flume::Receiver<PamMessage>, pw_tx: flume::Sender<String>) {
  let is_loading = futures_signals::signal::Mutable::new(false);
  let _ = locker::wayland::lock_session(SendApp(app.clone()), pw_tx, pam_rx, is_loading);
}

fn ctl_button(icon: &str) -> gtk4::Button {
  gtk4::Button::builder()
    .css_classes(["ctl-button"])
    .child(
      &gtk4::Image::builder()
        .resource(&format!("/lol/happens/dash3/icons/{icon}.svg"))
        .build(),
    )
    .build()
}

fn create_window(
  app: &gtk4::Application,
  is_loading: futures_signals::signal::Mutable<bool>,
  pw_tx: flume::Sender<String>,
  pam_rx: flume::Receiver<PamMessage>,
) -> gtk4::ApplicationWindow {
  let login = gtk4::Box::builder()
    .orientation(gtk4::Orientation::Vertical)
    .css_classes(["login-container"])
    .halign(gtk4::Align::Center)
    .spacing(24)
    .build();

  login.append(
    &gtk4::Image::builder()
      .resource("/lol/happens/dash3/profile.png")
      .overflow(gtk4::Overflow::Hidden)
      .halign(gtk4::Align::Center)
      .width_request(108)
      .height_request(108)
      .css_classes(["avatar"])
      .build(),
  );

  let input_container = gtk4::Box::builder()
    .orientation(gtk4::Orientation::Horizontal)
    .halign(gtk4::Align::Center)
    .css_classes(["login-input"])
    .spacing(12)
    .build();

  let input = gtk4::PasswordEntry::builder().width_chars(26).build();
  {
    let is_loading = is_loading.clone();
    let pw_tx = pw_tx.clone();
    input.connect_activate(move |input| {
      is_loading.set(true);
      pw_tx.send(input.text().to_string()).unwrap();
    });
  }

  let overlay = gtk4::Overlay::builder()
    .child(
      &gtk4::Image::builder()
        .resource("/lol/happens/dash3/icons/fingerprint-simple.svg")
        .build(),
    )
    .build();

  let spinner = gtk4::Spinner::new();
  overlay.add_overlay(&spinner);

  let input_button = gtk4::Button::builder()
    .css_classes(["login-button"])
    .child(&overlay)
    .build();

  {
    let is_loading = is_loading.clone();
    let pw_tx = pw_tx.clone();
    let input = input.downgrade();
    input_button.connect_clicked(move |_| {
      if let Some(input) = input.upgrade() {
        is_loading.set(true);
        pw_tx.send(input.text().to_string()).unwrap();
      }
    });
  }

  input_container.append(&input);
  input_container.append(&input_button);
  login.append(&input_container);

  let msg = gtk4::Label::builder().label("message").build();
  login.append(&msg);

  let ctl = gtk4::Box::builder()
    .orientation(gtk4::Orientation::Horizontal)
    .halign(gtk4::Align::Center)
    .css_classes(["ctl-container"])
    .spacing(10)
    .build();

  ctl.append(&ctl_button("arrow-clockwise"));
  ctl.append(&ctl_button("moon-stars"));
  ctl.append(&ctl_button("power"));

  let root = gtk4::CenterBox::builder()
    .orientation(gtk4::Orientation::Vertical)
    .halign(gtk4::Align::Center)
    .center_widget(&login)
    .end_widget(&ctl)
    .build();

  // root.set_sensitive(false);
  // root.set_visible(false);

  let window = ApplicationWindow::builder()
    .application(app)
    .css_classes(["window"])
    .child(&root)
    .build();

  {
    let is_loading = is_loading.clone();
    let app = app.downgrade();
    glib::spawn_future_local(async move {
      while let Ok(pam_rx) = pam_rx.recv_async().await {
        match pam_rx {
          PamMessage::Echo(s) => {
            info!("echo: {s}");
            is_loading.set(false);
          }
          PamMessage::Blind(s) => {
            info!("blind: {s}");
            is_loading.set(false);
          }
          PamMessage::Info(s) => info!("info: {s}"),
          PamMessage::Error(s) => info!("error: {s}"),
          PamMessage::Success => {
            if let Some(app) = app.upgrade() {
              app.quit();
            }
          }
        }
      }
    });
  }

  glib::spawn_future_local(is_loading.signal().for_each(move |is_loading| {
    let input = input.downgrade();
    let input_button = input_button.downgrade();
    let spinner = spinner.downgrade();

    async move {
      if is_loading {
        if let Some(input) = input.upgrade() {
          input.set_sensitive(false);
        }

        if let Some(input_button) = input_button.upgrade() {
          input_button.set_sensitive(false);
          input_button.add_css_class("loading");
        }

        if let Some(spinner) = spinner.upgrade() {
          spinner.start();
        }
      } else {
        if let Some(input) = input.upgrade() {
          input.set_sensitive(true);
          input.grab_focus();
        }

        if let Some(input_button) = input_button.upgrade() {
          input_button.set_sensitive(true);
          input_button.remove_css_class("loading");
        }

        if let Some(spinner) = spinner.upgrade() {
          spinner.stop();
        }
      }
    }
  }));

  window
}
