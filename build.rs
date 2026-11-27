// build.rs
fn main() {
    slint_build::compile("ui/app-window.slint").expect("Error al compilar la UI de Slint");
}
