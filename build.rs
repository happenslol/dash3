fn main() {
  glib_build_tools::compile_resources(
    &["resources"],
    "resources/resources.gresource.xml",
    "dash3.gresource",
  );
}
