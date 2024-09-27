use gtk4::{
  cairo::{RectangleInt, Region},
  gdk::Display,
  glib,
  prelude::*,
  style_context_add_provider_for_display, Application, ApplicationWindow, CssProvider,
  STYLE_PROVIDER_PRIORITY_APPLICATION,
};
use gtk4_layer_shell::{Edge, Layer, LayerShell};

fn main() -> glib::ExitCode {
  let app = Application::builder()
    .application_id("lol.happens.test")
    .build();

  app.connect_startup(|_| {
    let provider = CssProvider::new();
    provider.load_from_string(include_str!("styles.css"));

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
  let bbox = gtk4::Box::builder()
    .width_request(100)
    .height_request(100)
    .css_classes(["interactive"])
    .build();

  let window = ApplicationWindow::builder()
    .application(app)
    .default_width(400)
    .default_height(200)
    .child(&bbox)
    .build();

  window.init_layer_shell();
  window.set_layer(Layer::Overlay);
  window.set_anchor(Edge::Top, true);
  window.set_anchor(Edge::Right, true);
  window.set_anchor(Edge::Bottom, true);
  window.set_anchor(Edge::Left, true);
  window.add_css_class("root");

  window.connect_show(|window| {
    let s = window.surface().unwrap();
    s.set_input_region(&Region::create_rectangle(&RectangleInt::new(
      0, 0, 100, 100,
    )));
  });

  window.present();
}
