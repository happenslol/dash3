use std::{path::PathBuf, thread};

use gtk4::{
  gdk::Display,
  glib::{self, clone},
  prelude::*,
  style_context_add_provider_for_display, Application, ApplicationWindow, CssProvider,
  STYLE_PROVIDER_PRIORITY_APPLICATION,
};
use notify::Watcher;

mod pam;
mod scrambler;

enum Action {
  Login(String),
}

#[derive(Debug, Clone)]
enum Update {
  ShowMessage(String),
}

fn load_css() -> String {
  grass::from_path("./src/styles.scss", &grass::Options::default()).unwrap()
}

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

    let icon_theme = gtk4::IconTheme::for_display(&display);
    icon_theme.add_resource_path("/lol/happens/dash3/icons");

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

  let (action_tx, action_rx) = flume::unbounded::<Action>();
  let (update_tx, update_rx) = async_broadcast::broadcast(10);

  let pam_action_rx = action_rx.clone();
  let pam_update_tx = update_tx.clone();
  thread::spawn(move || loop {
    let (pam_tx, pam_rx) = flume::unbounded::<pam::PamRequest>();
    let (pw_tx, pw_rx) = flume::unbounded::<String>();

    let conv = pam::ChannelConv::new(pw_rx, pam_tx);
    let conv = Box::pin(conv);
    let mut pam_session = pam::session::PamSession::start("dash3", "happens", conv).unwrap();

    let select_action_rx = pam_action_rx.clone();
    let select_update_tx = pam_update_tx.clone();
    let (done_tx, done_rx) = flume::unbounded::<()>();

    let handle = thread::spawn(move || loop {
      let should_continue = flume::Selector::new()
        .recv(&select_action_rx, |action| match action {
          Ok(action) => {
            match action {
              Action::Login(s) => pw_tx.send(s).unwrap(),
            };

            true
          }
          Err(flume::RecvError::Disconnected) => false,
        })
        .recv(&pam_rx, |req| match req {
          Ok(req) => match req {
            pam::PamRequest::Echo(s)
            | pam::PamRequest::Blind(s)
            | pam::PamRequest::Info(s)
            | pam::PamRequest::Error(s) => {
              futures::executor::block_on(select_update_tx.broadcast(Update::ShowMessage(s)))
                .unwrap();

              true
            }
          },
          Err(flume::RecvError::Disconnected) => false,
        })
        .recv(&done_rx, |_| false)
        .wait();

      if !should_continue {
        break;
      }
    });

    match pam_session.authenticate(pam_sys::PamFlag::NONE) {
      Ok(()) => {
        futures::executor::block_on(
          pam_update_tx.broadcast(Update::ShowMessage("Success!".to_string())),
        )
        .unwrap();
      }
      Err(err) => {
        futures::executor::block_on(pam_update_tx.broadcast(Update::ShowMessage(err.to_string())))
          .unwrap();
      }
    }

    thread::sleep(std::time::Duration::from_secs(3));
    let _ = done_tx.send(());
    let _ = handle.join();
  });

  app.connect_activate(move |app| build_ui(app, action_tx.clone(), update_rx.clone()));
  app.run()
}

fn build_ui(
  app: &Application,
  action_tx: flume::Sender<Action>,
  mut update_rx: async_broadcast::Receiver<Update>,
) {
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
  let move_action_tx = action_tx.clone();
  input.connect_activate(move |input| {
    move_action_tx
      .send(Action::Login(input.text().to_string()))
      .unwrap();
  });

  let input_button = gtk4::Button::builder()
    .css_classes(["login-button"])
    .child(
      &gtk4::Image::builder()
        .resource("/lol/happens/dash3/icons/fingerprint-simple.svg")
        .build(),
    )
    .build();

  let move_action_tx = action_tx.clone();
  input_button.connect_clicked(clone!(
    #[weak]
    input,
    move |_| {
      move_action_tx
        .send(Action::Login(input.text().to_string()))
        .unwrap();
    },
  ));

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

  let window = ApplicationWindow::builder()
    .application(app)
    .css_classes(["window"])
    .child(&root)
    .build();

  // window.init_layer_shell();
  // window.set_layer(Layer::Overlay);
  // window.set_anchor(Edge::Top, true);
  // window.set_anchor(Edge::Right, true);

  glib::spawn_future_local(async move {
    while let Ok(update) = update_rx.recv().await {
      match update {
        Update::ShowMessage(s) => msg.set_label(&s),
      }
    }
  });

  window.present();
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
