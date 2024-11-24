use gtk4::{
  gdk::Display, glib, prelude::*, style_context_add_provider_for_display, Application,
  ApplicationWindow, Box, CssProvider, STYLE_PROVIDER_PRIORITY_APPLICATION,
};
use gtk4_layer_shell::{Edge, Layer, LayerShell};

static CSS: &str = grass::include!("src/styles.scss");

fn main() -> glib::ExitCode {
  let app = Application::builder()
    .application_id("lol.happens.test")
    .build();

  app.connect_startup(|_| {
    let provider = CssProvider::new();
    provider.load_from_string(CSS);

    style_context_add_provider_for_display(
      &Display::default().unwrap(),
      &provider,
      STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
  });

  app.connect_activate(build_ui);
  app.run()
}

fn build_ui(app: &Application) {
  let login = gtk4::Box::builder()
    .orientation(gtk4::Orientation::Vertical)
    .halign(gtk4::Align::Center)
    .spacing(10)
    .build();

  login.append(
    &gtk4::Image::builder()
      .halign(gtk4::Align::Center)
      .width_request(120)
      .height_request(120)
      .css_classes(["avatar"])
      .build(),
  );

  let input = gtk4::Box::builder()
    .orientation(gtk4::Orientation::Horizontal)
    .halign(gtk4::Align::Center)
    .spacing(10)
    .build();

  input.append(&gtk4::PasswordEntry::builder().width_chars(32).build());

  input.append(
    &gtk4::Button::builder()
      .css_classes(["login-button"])
      .build(),
  );

  login.append(&input);

  let ctl = gtk4::Box::builder()
    .orientation(gtk4::Orientation::Horizontal)
    .halign(gtk4::Align::Center)
    .spacing(10)
    .build();

  ctl.append(&gtk4::Button::builder().css_classes(["ctl-button"]).build());
  ctl.append(&gtk4::Button::builder().css_classes(["ctl-button"]).build());
  ctl.append(&gtk4::Button::builder().css_classes(["ctl-button"]).build());

  let root = gtk4::CenterBox::builder()
    .orientation(gtk4::Orientation::Vertical)
    .halign(gtk4::Align::Center)
    .center_widget(&login)
    .end_widget(&ctl)
    .build();

  let window = ApplicationWindow::builder()
    .application(app)
    .default_width(600)
    .default_height(600)
    .css_classes(["window"])
    .child(&root)
    .build();

  // window.init_layer_shell();
  // window.set_layer(Layer::Overlay);
  // window.set_anchor(Edge::Top, true);
  // window.set_anchor(Edge::Right, true);

  window.present();
}
